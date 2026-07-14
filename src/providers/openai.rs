use anyhow::anyhow;
use async_stream::stream;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::json;

use crate::models::chat::{
    ChatRequest, ChatResponse, ContentPart, FinishReason, Message, ProviderStream, Role,
    StreamEvent, ToolCall, Usage,
};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::providers::provider::{ModelInfo, Provider};

pub struct OpenAIProvider {
    client: Client,
    base_url: String,
    label: &'static str,
}

impl OpenAIProvider {
    pub const NAME: &'static str = "openai";

    pub fn new() -> Self {
        Self::compatible("OpenAI", "https://api.openai.com/v1".to_string())
    }

    pub(crate) fn compatible(label: &'static str, base_url: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap(),
            base_url,
            label,
        }
    }

    fn get_model(&self, req: &ChatRequest) -> anyhow::Result<String> {
        req.model
            .clone()
            .ok_or_else(|| anyhow!("No model specified"))
    }

    fn get_embedding_model(&self, req: &EmbeddingRequest) -> anyhow::Result<String> {
        req.model
            .clone()
            .ok_or_else(|| anyhow!("No embedding model specified"))
    }

    fn serialize_messages(&self, messages: &[Message]) -> Vec<serde_json::Value> {
        messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };

                let content: String = m
                    .content
                    .iter()
                    .filter_map(|p| match p {
                        ContentPart::Text { text } => Some(text.clone()),
                    })
                    .collect::<Vec<_>>()
                    .join("");

                let mut msg = json!({"role": role, "content": content});

                if !m.tool_calls.is_empty() {
                    msg["tool_calls"] = json!(
                        m.tool_calls
                            .iter()
                            .map(|tc| json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {"name": tc.name, "arguments": tc.arguments}
                            }))
                            .collect::<Vec<_>>()
                    );
                }

                if let Some(id) = &m.tool_call_id {
                    msg["tool_call_id"] = json!(id);
                }

                msg
            })
            .collect()
    }

    fn serialize_tools(&self, tools: &[crate::models::chat::Tool]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect()
    }

    fn serialize_tool_choice(&self, choice: &crate::models::chat::ToolChoice) -> serde_json::Value {
        match choice {
            crate::models::chat::ToolChoice::Auto => json!("auto"),
            crate::models::chat::ToolChoice::None => json!("none"),
            crate::models::chat::ToolChoice::Required => json!("required"),
            crate::models::chat::ToolChoice::Tool(name) => json!({
                "type": "function",
                "function": {"name": name}
            }),
        }
    }

    fn apply_reasoning_fields(&self, body: &mut serde_json::Value, req: &ChatRequest) {
        if let Some(effort) = &req.reasoning_effort {
            body["reasoning_effort"] = json!(effort.as_str());
        }

        // OpenAI Chat Completions does not accept Anthropic-style `thinking` object.
        // We intentionally ignore `thinking_budget_tokens` here for provider compatibility.
    }
}

#[async_trait::async_trait]
impl Provider for OpenAIProvider {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    async fn chat(&self, req: ChatRequest, api_key: String) -> anyhow::Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = json!({
            "model": self.get_model(&req)?,
            "messages": self.serialize_messages(&req.messages),
            "temperature": req.temperature.unwrap_or(1.0),
            "top_p": req.top_p.unwrap_or(1.0),
            "stream": false,
        });

        if let Some(max_tokens) = req.max_tokens {
            body["max_completion_tokens"] = json!(max_tokens);
        }
        if !req.tools.is_empty() {
            body["tools"] = json!(self.serialize_tools(&req.tools));
        }
        if let Some(choice) = &req.tool_choice {
            body["tool_choice"] = self.serialize_tool_choice(choice);
        }

        self.apply_reasoning_fields(&mut body, &req);

        let res = self
            .client
            .post(&url)
            .bearer_auth(&api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await?;
            return Err(anyhow!("{} returned {}: {}", self.label, status, body));
        }

        let json: serde_json::Value = res.json().await?;
        let choice = &json["choices"][0];
        let msg = &choice["message"];

        let content = msg["content"].as_str().unwrap_or("").to_string();
        let tool_calls = msg["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .map(|c| ToolCall {
                        id: c["id"].as_str().unwrap_or("").to_string(),
                        name: c["function"]["name"].as_str().unwrap_or("").to_string(),
                        arguments: c["function"]["arguments"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let finish_reason = match choice["finish_reason"].as_str().unwrap_or("stop") {
            "tool_calls" => FinishReason::ToolCalls,
            "length" => FinishReason::Length,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Stop,
        };

        let usage = json.get("usage").map(|u| Usage {
            prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            message: Message {
                role: Role::Assistant,
                content: vec![ContentPart::Text { text: content }],
                tool_calls,
                tool_call_id: None,
            },
            finish_reason,
            usage,
        })
    }

    async fn chat_stream(
        &self,
        req: ChatRequest,
        api_key: String,
    ) -> anyhow::Result<ProviderStream> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = json!({
            "model": self.get_model(&req)?,
            "messages": self.serialize_messages(&req.messages),
            "temperature": req.temperature.unwrap_or(1.0),
            "top_p": req.top_p.unwrap_or(1.0),
            "stream": true,
        });

        if let Some(max_tokens) = req.max_tokens {
            body["max_completion_tokens"] = json!(max_tokens);
        }
        if !req.tools.is_empty() {
            body["tools"] = json!(self.serialize_tools(&req.tools));
        }
        if let Some(choice) = &req.tool_choice {
            body["tool_choice"] = self.serialize_tool_choice(choice);
        }

        self.apply_reasoning_fields(&mut body, &req);

        let res = self
            .client
            .post(&url)
            .bearer_auth(&api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await?;
            return Err(anyhow!("{} returned {}: {}", self.label, status, body));
        }

        let mut sse = res.bytes_stream().eventsource();
        let mut tool_state: std::collections::HashMap<usize, (String, bool)> =
            std::collections::HashMap::new();

        let s = stream! {
            while let Some(event_result) = sse.next().await {
                let event = match event_result {
                    Ok(e) => e,
                    Err(e) => {
                        yield Err(anyhow!("SSE error: {}", e));
                        continue;
                    }
                };

                if event.data == "[DONE]" {
                    for (_, (_id, name_seen)) in tool_state.drain() {
                        if name_seen {
                            yield Ok(StreamEvent::ToolCallFinished);
                        }
                    }
                    yield Ok(StreamEvent::Done);
                    return;
                }

                let json: serde_json::Value = match serde_json::from_str(&event.data) {
                    Ok(v) => v,
                    Err(e) => {
                        yield Ok(StreamEvent::Error { message: format!("Parse error: {}", e) });
                        continue;
                    }
                };

                let delta = &json["choices"][0]["delta"];
                let finish_reason = json["choices"][0]["finish_reason"].as_str();

                if let Some(content) = delta["content"].as_str() {
                    if !content.is_empty() {
                        yield Ok(StreamEvent::TextDelta(content.to_string()));
                    }
                }

                if let Some(tool_calls) = delta["tool_calls"].as_array() {
                    for call in tool_calls {
                        let index = call["index"].as_u64().unwrap_or(0) as usize;
                        let id = call["id"].as_str().unwrap_or("").to_string();
                        let name = call["function"]["name"].as_str().unwrap_or("").to_string();
                        let args = call["function"]["arguments"]
                            .as_str()
                            .unwrap_or("")
                            .to_string();

                        let entry = tool_state.entry(index).or_insert_with(|| (id.clone(), false));
                        if !id.is_empty() && entry.0.is_empty() {
                            entry.0 = id;
                        }
                        if !name.is_empty() && !entry.1 {
                            entry.1 = true;
                            yield Ok(StreamEvent::ToolCallStarted {
                                id: entry.0.clone(),
                                name,
                            });
                        }
                        if !args.is_empty() {
                            yield Ok(StreamEvent::ToolCallDelta {
                                id: entry.0.clone(),
                                arguments_chunk: args,
                            });
                        }
                    }
                }

                if finish_reason == Some("tool_calls") {
                    for (_, (_id, name_seen)) in tool_state.drain() {
                        if name_seen {
                            yield Ok(StreamEvent::ToolCallFinished);
                        }
                    }
                }

                if let Some(usage) = json.get("usage") {
                    if let (Some(p), Some(c), Some(t)) = (
                        usage["prompt_tokens"].as_u64(),
                        usage["completion_tokens"].as_u64(),
                        usage["total_tokens"].as_u64(),
                    ) {
                        yield Ok(StreamEvent::Usage(Usage {
                            prompt_tokens: p as u32,
                            completion_tokens: c as u32,
                            total_tokens: t as u32,
                        }));
                    }
                }
            }
        };

        Ok(Box::pin(s))
    }

    async fn embeddings(
        &self,
        req: EmbeddingRequest,
        api_key: String,
    ) -> anyhow::Result<EmbeddingResponse> {
        let url = format!("{}/embeddings", self.base_url);

        let body = json!({
            "model": self.get_embedding_model(&req)?,
            "input": req.input,
        });

        let res = self
            .client
            .post(&url)
            .bearer_auth(&api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await?;
            return Err(anyhow!("{} returned {}: {}", self.label, status, body));
        }

        let json: serde_json::Value = res.json().await?;
        let vectors = json["data"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid OpenAI embeddings response: missing data array"))?
            .iter()
            .map(|item| {
                item["embedding"]
                    .as_array()
                    .ok_or_else(|| anyhow!("Invalid OpenAI embeddings response: missing embedding"))
                    .map(|arr| {
                        arr.iter()
                            .map(|v| v.as_f64().unwrap_or(0.0))
                            .collect::<Vec<f64>>()
                    })
            })
            .collect::<anyhow::Result<Vec<Vec<f64>>>>()?;

        Ok(EmbeddingResponse { vectors })
    }

    async fn models(&self, api_key: String) -> anyhow::Result<Vec<ModelInfo>> {
        let res = self
            .client
            .get(format!("{}/models", self.base_url))
            .bearer_auth(api_key)
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            return Err(anyhow!(
                "{} returned {}: {}",
                self.label,
                status,
                res.text().await?
            ));
        }
        let json: serde_json::Value = res.json().await?;
        Ok(json["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| model["id"].as_str())
            .map(ModelInfo::unknown)
            .collect())
    }
}
