use anyhow::{Result, anyhow};
use async_stream::stream;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use reqwest::Client;

use crate::models::chat::{
    ChatRequest, ChatResponse, ContentPart, FinishReason, Message, ProviderStream, Role,
    StreamEvent, Usage,
};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::providers::provider::Provider;

pub struct NimProvider {
    pub client: Client,
    pub base_url: String,
}

impl NimProvider {
    pub const NAME: &'static str = "nvidia-nim";

    pub fn new() -> Self {
        Self {
            client: Client::new(),
            base_url: "https://integrate.api.nvidia.com/v1".to_string(),
        }
    }

    fn get_model(&self, req: &ChatRequest) -> Result<String> {
        req.model
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No model configured. Run: nimbox model <model-name>"))
    }

    fn get_embedding_model(&self, req: &EmbeddingRequest) -> Result<String> {
        req.model
            .clone()
            .ok_or_else(|| anyhow::anyhow!("No model configured. Run: nimbox embed <model-name>"))
    }

    fn serialize_tools(&self, tools: &[crate::models::chat::Tool]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .map(|tool| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": tool.parameters
                    }
                })
            })
            .collect()
    }

    fn serialize_tool_choice(&self, choice: &crate::models::chat::ToolChoice) -> serde_json::Value {
        use crate::models::chat::ToolChoice;

        match choice {
            ToolChoice::Auto => serde_json::json!("auto"),

            ToolChoice::None => serde_json::json!("none"),

            ToolChoice::Required => serde_json::json!("required"),

            ToolChoice::Tool(name) => serde_json::json!({
                "type": "function",
                "function": {
                    "name": name
                }
            }),
        }
    }

    fn serialize_messages(
        &self,
        messages: &[crate::models::chat::Message],
    ) -> Vec<serde_json::Value> {
        use crate::models::chat::{ContentPart, Role};

        messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };

                let text = message
                    .content
                    .iter()
                    .filter_map(|part| match part {
                        ContentPart::Text { text } => Some(text.clone()),
                    })
                    .collect::<Vec<_>>()
                    .join("");

                let mut value = serde_json::json!({
                    "role": role,
                    "content": text,
                });

                if role == "tool" {
                    value["tool_call_id"] = serde_json::json!(message.tool_call_id);
                }

                if !message.tool_calls.is_empty() {
                    value["tool_calls"] = serde_json::Value::Array(
                        message
                            .tool_calls
                            .iter()
                            .map(|call| {
                                serde_json::json!({
                                    "id": call.id,
                                    "type": "function",
                                    "function": {
                                        "name": call.name,
                                        "arguments": call.arguments
                                    }
                                })
                            })
                            .collect(),
                    );
                }

                value
            })
            .collect()
    }

    fn parse_tool_calls(&self, json: &serde_json::Value) -> Vec<crate::models::chat::ToolCall> {
        json["choices"][0]["message"]["tool_calls"]
            .as_array()
            .map(|calls| {
                calls
                    .iter()
                    .map(|call| crate::models::chat::ToolCall {
                        id: call["id"].as_str().unwrap_or("").to_string(),

                        name: call["function"]["name"].as_str().unwrap_or("").to_string(),

                        arguments: call["function"]["arguments"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl Provider for NimProvider {
    async fn chat(&self, req: ChatRequest, api_key: String) -> Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let mut body = serde_json::json!({
            "model": self.get_model(&req)?,
            "messages": self.serialize_messages(&req.messages),
            "max_tokens": req.max_tokens.unwrap_or(131072),
            "temperature": req.temperature.unwrap_or(1.0),
            "top_p": req.top_p.unwrap_or(1.0),
            "stream": false,
            "reasoning_budget": 32768,
            "chat_template_kwargs": {"enable_thinking": true},
        });

        if !req.tools.is_empty() {
            body["tools"] = serde_json::Value::Array(self.serialize_tools(&req.tools));
        }

        if let Some(choice) = &req.tool_choice {
            body["tool_choice"] = self.serialize_tool_choice(choice);
        }

        let res = self
            .client
            .post(url)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = res.status();

        if !status.is_success() {
            let body = res.text().await?;

            return Err(anyhow!("NVIDIA NIM returned {}: {}", status, body));
        }

        let json = res.json::<serde_json::Value>().await?;
        let content = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();

        let tool_calls = self.parse_tool_calls(&json);
        let message = Message {
            role: Role::Assistant,
            content: vec![ContentPart::Text { text: content }],
            tool_calls,
            tool_call_id: None,
        };
        let finish_reason = match json["choices"][0]["finish_reason"].as_str().unwrap_or("") {
            "stop" => FinishReason::Stop,
            "length" => FinishReason::Length,
            "tool_calls" => FinishReason::ToolCalls,
            "content_filter" => FinishReason::ContentFilter,
            _ => FinishReason::Unknown,
        };
        let usage = json.get("usage").map(|usage| Usage {
            prompt_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: usage["completion_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: usage["total_tokens"].as_u64().unwrap_or(0) as u32,
        });
        Ok(ChatResponse {
            message,
            finish_reason,
            usage,
        })
    }

    async fn chat_stream(&self, req: ChatRequest, api_key: String) -> Result<ProviderStream> {
        let url = format!("{}/chat/completions", self.base_url);
        let mut body = serde_json::json!({
            "model": self.get_model(&req)?,
            "messages": self.serialize_messages(&req.messages),
            "max_tokens": req.max_tokens.unwrap_or(131072),
            "temperature": req.temperature.unwrap_or(1.0),
            "top_p": req.top_p.unwrap_or(1.0),
            "stream": true,
            "reasoning_budget": 32768,
            "chat_template_kwargs": {"enable_thinking": true},
        });
        if !req.tools.is_empty() {
            body["tools"] = serde_json::Value::Array(self.serialize_tools(&req.tools));
        }
        if let Some(choice) = &req.tool_choice {
            body["tool_choice"] = self.serialize_tool_choice(choice);
        }

        let res = self
            .client
            .post(url)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await?;

        let status = res.status();
        if !status.is_success() {
            let body = res.text().await?;
            return Err(anyhow!("NVIDIA NIM returned {}: {}", status, body));
        }

        let mut sse = res.bytes_stream().eventsource();
        let mut tool_state: std::collections::HashMap<usize, (String, bool)> =
            std::collections::HashMap::new();

        let s = stream! {
            while let Some(event_result) = sse.next().await {
                let event = match event_result {
                    Ok(e) => e,
                    Err(e) => {
                        yield Ok(StreamEvent::Error { message: format!("Parse error: {}", e) });
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
                    Err(_) => continue,
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
                            entry.0 = id.clone();
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
    ) -> Result<EmbeddingResponse> {
        let url = format!("{}/embeddings", self.base_url);

        let body = serde_json::json!({
            "input": req.input,
            "model": self.get_embedding_model(&req)?,
            "input_type": "query",
            "encoding_format": "float",
            "truncate": "NONE"
        });

        let res = self
            .client
            .post(url)
            .bearer_auth(api_key)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = res.status();

        if !status.is_success() {
            let body = res.text().await?;

            return Err(anyhow!("NVIDIA NIM returned {}: {}", status, body));
        }

        let json = res.json::<serde_json::Value>().await?;
        let vectors = json["data"]
            .as_array()
            .ok_or_else(|| anyhow!("Missing data array"))?
            .iter()
            .map(|item| {
                item["embedding"]
                    .as_array()
                    .ok_or_else(|| anyhow!("Missing embedding array"))?
                    .iter()
                    .map(|v| {
                        v.as_f64()
                            .ok_or_else(|| anyhow!("Embedding contains non-float value"))
                    })
                    .collect::<Result<Vec<f64>>>()
            })
            .collect::<Result<Vec<Vec<f64>>>>()?;

        Ok(EmbeddingResponse { vectors })
    }
}
