use anyhow::{Result, anyhow};
use async_stream::stream;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::models::chat::{
    ChatRequest, ChatResponse, ContentPart, FinishReason, Message, ProviderStream, Role,
    StreamEvent, ToolCall, ToolChoice, Usage,
};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::providers::provider::{ModelInfo, Provider};

pub struct AiStudioProvider {
    client: reqwest::Client,
    base_url: String,
}

impl AiStudioProvider {
    pub const NAME: &'static str = "ai-studio";

    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("valid HTTP client"),
            base_url: std::env::var("NIMBOX_AI_STUDIO_URL")
                .unwrap_or_else(|_| "https://generativelanguage.googleapis.com/v1beta".to_string()),
        }
    }

    fn model_name(model: &str) -> &str {
        model.strip_prefix("models/").unwrap_or(model)
    }

    fn get_chat_model(req: &ChatRequest) -> Result<&str> {
        req.model
            .as_deref()
            .map(Self::model_name)
            .ok_or_else(|| anyhow!("No model specified"))
    }

    fn get_embedding_model(req: &EmbeddingRequest) -> Result<&str> {
        req.model
            .as_deref()
            .map(Self::model_name)
            .ok_or_else(|| anyhow!("No embedding model specified"))
    }

    fn text(message: &Message) -> String {
        message
            .content
            .iter()
            .map(|part| match part {
                ContentPart::Text { text } => text.as_str(),
            })
            .collect()
    }

    fn request_body(req: &ChatRequest) -> Value {
        let system = req
            .messages
            .iter()
            .filter(|message| matches!(message.role, Role::System))
            .map(Self::text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");

        let tool_names = req
            .messages
            .iter()
            .flat_map(|message| &message.tool_calls)
            .map(|call| (call.id.as_str(), call.name.as_str()))
            .collect::<std::collections::HashMap<_, _>>();

        let contents = req
            .messages
            .iter()
            .filter(|message| !matches!(message.role, Role::System))
            .map(|message| {
                if matches!(message.role, Role::Tool) {
                    let name = message
                        .tool_call_id
                        .as_deref()
                        .and_then(|id| tool_names.get(id).copied())
                        .unwrap_or("tool");
                    let text = Self::text(message);
                    let response = serde_json::from_str::<Value>(&text)
                        .ok()
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({ "result": text }));
                    return json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": { "name": name, "response": response }
                        }]
                    });
                }

                let mut parts = Vec::new();
                let text = Self::text(message);
                if !text.is_empty() {
                    parts.push(json!({ "text": text }));
                }
                parts.extend(message.tool_calls.iter().map(|call| {
                    let args = serde_json::from_str::<Value>(&call.arguments)
                        .unwrap_or_else(|_| json!({}));
                    json!({
                        "functionCall": {
                            "id": call.id,
                            "name": call.name,
                            "args": args
                        }
                    })
                }));
                json!({
                    "role": if matches!(message.role, Role::Assistant) { "model" } else { "user" },
                    "parts": parts
                })
            })
            .collect::<Vec<_>>();

        let mut body = json!({ "contents": contents });
        if !system.is_empty() {
            body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
        }
        if !req.tools.is_empty() {
            body["tools"] = json!([{
                "functionDeclarations": req.tools.iter().map(|tool| json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters
                })).collect::<Vec<_>>()
            }]);
        }
        if let Some(choice) = &req.tool_choice {
            let (mode, allowed): (&str, Option<Vec<&str>>) = match choice {
                ToolChoice::Auto => ("AUTO", None),
                ToolChoice::None => ("NONE", None),
                ToolChoice::Required => ("ANY", None),
                ToolChoice::Tool(name) => ("ANY", Some(vec![name.as_str()])),
            };
            let mut function_config = json!({ "mode": mode });
            if let Some(allowed) = allowed {
                function_config["allowedFunctionNames"] = json!(allowed);
            }
            body["toolConfig"] = json!({ "functionCallingConfig": function_config });
        }

        let mut generation = json!({});
        if let Some(value) = req.max_tokens {
            generation["maxOutputTokens"] = json!(value);
        }
        if let Some(value) = req.temperature {
            generation["temperature"] = json!(value);
        }
        if let Some(value) = req.top_p {
            generation["topP"] = json!(value);
        }
        if let Some(value) = req.thinking_budget_tokens {
            generation["thinkingConfig"] = json!({ "thinkingBudget": value });
        } else if let Some(effort) = &req.reasoning_effort {
            generation["thinkingConfig"] = json!({
                "thinkingLevel": effort.as_str().to_ascii_uppercase()
            });
        }
        if generation.as_object().is_some_and(|map| !map.is_empty()) {
            body["generationConfig"] = generation;
        }
        body
    }

    fn parse_response(json: &Value) -> Result<ChatResponse> {
        let candidate = json["candidates"]
            .as_array()
            .and_then(|candidates| candidates.first())
            .ok_or_else(|| anyhow!("Gemini returned no candidates: {}", json["promptFeedback"]))?;
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        for (index, part) in candidate["content"]["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            if let Some(value) = part["text"].as_str() {
                text.push_str(value);
            }
            if let Some(call) = part.get("functionCall") {
                tool_calls.push(ToolCall {
                    id: call["id"]
                        .as_str()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| format!("call_{index}")),
                    name: call["name"].as_str().unwrap_or("").to_string(),
                    arguments: call["args"].to_string(),
                });
            }
        }
        let finish_reason = match candidate["finishReason"].as_str().unwrap_or("STOP") {
            "MAX_TOKENS" => FinishReason::Length,
            "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII"
            | "IMAGE_SAFETY" => FinishReason::ContentFilter,
            "MALFORMED_FUNCTION_CALL" => FinishReason::Error,
            _ if !tool_calls.is_empty() => FinishReason::ToolCalls,
            "STOP" => FinishReason::Stop,
            _ => FinishReason::Unknown,
        };
        let usage = json.get("usageMetadata").map(|usage| Usage {
            prompt_tokens: usage["promptTokenCount"].as_u64().unwrap_or(0) as u32,
            completion_tokens: usage["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
            total_tokens: usage["totalTokenCount"].as_u64().unwrap_or(0) as u32,
        });
        Ok(ChatResponse {
            message: Message {
                role: Role::Assistant,
                content: vec![ContentPart::Text { text }],
                tool_calls,
                tool_call_id: None,
            },
            finish_reason,
            usage,
        })
    }

    async fn checked_json(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "Google AI Studio returned {}: {}",
                status,
                response.text().await?
            ));
        }
        Ok(response.json().await?)
    }
}

#[async_trait::async_trait]
impl Provider for AiStudioProvider {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    async fn chat(&self, req: ChatRequest, api_key: String) -> Result<ChatResponse> {
        let model = Self::get_chat_model(&req)?;
        let json = self
            .checked_json(
                self.client
                    .post(format!("{}/models/{model}:generateContent", self.base_url))
                    .header("x-goog-api-key", api_key)
                    .json(&Self::request_body(&req)),
            )
            .await?;
        Self::parse_response(&json)
    }

    async fn chat_stream(&self, req: ChatRequest, api_key: String) -> Result<ProviderStream> {
        let model = Self::get_chat_model(&req)?;
        let response = self
            .client
            .post(format!(
                "{}/models/{model}:streamGenerateContent?alt=sse",
                self.base_url
            ))
            .header("x-goog-api-key", api_key)
            .json(&Self::request_body(&req))
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                "Google AI Studio returned {}: {}",
                status,
                response.text().await?
            ));
        }

        let mut events = response.bytes_stream().eventsource();
        let output = stream! {
            let mut call_index = 0usize;
            while let Some(event) = events.next().await {
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        yield Ok(StreamEvent::Error { message: error.to_string() });
                        continue;
                    }
                };
                let json: Value = match serde_json::from_str(&event.data) {
                    Ok(json) => json,
                    Err(error) => {
                        yield Ok(StreamEvent::Error { message: error.to_string() });
                        continue;
                    }
                };
                for part in json["candidates"][0]["content"]["parts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    if let Some(text) = part["text"].as_str().filter(|text| !text.is_empty()) {
                        yield Ok(StreamEvent::TextDelta(text.to_string()));
                    }
                    if let Some(call) = part.get("functionCall") {
                        let id = call["id"]
                            .as_str()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| format!("call_{call_index}"));
                        call_index += 1;
                        yield Ok(StreamEvent::ToolCallStarted {
                            id: id.clone(),
                            name: call["name"].as_str().unwrap_or("").to_string(),
                        });
                        yield Ok(StreamEvent::ToolCallDelta {
                            id,
                            arguments_chunk: call["args"].to_string(),
                        });
                        yield Ok(StreamEvent::ToolCallFinished);
                    }
                }
                if let Some(usage) = json.get("usageMetadata") {
                    yield Ok(StreamEvent::Usage(Usage {
                        prompt_tokens: usage["promptTokenCount"].as_u64().unwrap_or(0) as u32,
                        completion_tokens: usage["candidatesTokenCount"].as_u64().unwrap_or(0) as u32,
                        total_tokens: usage["totalTokenCount"].as_u64().unwrap_or(0) as u32,
                    }));
                }
            }
            yield Ok(StreamEvent::Done);
        };
        Ok(Box::pin(output))
    }

    async fn embeddings(
        &self,
        req: EmbeddingRequest,
        api_key: String,
    ) -> Result<EmbeddingResponse> {
        let model = Self::get_embedding_model(&req)?;
        let model_path = format!("models/{model}");
        let requests = req
            .input
            .iter()
            .map(|text| {
                json!({
                    "model": model_path,
                    "content": { "parts": [{ "text": text }] }
                })
            })
            .collect::<Vec<_>>();
        let json = self
            .checked_json(
                self.client
                    .post(format!(
                        "{}/models/{model}:batchEmbedContents",
                        self.base_url
                    ))
                    .header("x-goog-api-key", api_key)
                    .json(&json!({ "requests": requests })),
            )
            .await?;
        let vectors = json["embeddings"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid Gemini embeddings response"))?
            .iter()
            .map(|embedding| {
                embedding["values"]
                    .as_array()
                    .ok_or_else(|| anyhow!("Gemini embedding is missing values"))?
                    .iter()
                    .map(|value| {
                        value
                            .as_f64()
                            .ok_or_else(|| anyhow!("Gemini embedding contains a non-number"))
                    })
                    .collect()
            })
            .collect::<Result<Vec<Vec<f64>>>>()?;
        Ok(EmbeddingResponse { vectors })
    }

    async fn models(&self, api_key: String) -> Result<Vec<ModelInfo>> {
        let json = self
            .checked_json(
                self.client
                    .get(format!("{}/models?pageSize=1000", self.base_url))
                    .header("x-goog-api-key", api_key),
            )
            .await?;
        Ok(json["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| {
                let id = model["name"].as_str()?.strip_prefix("models/")?;
                let methods = model["supportedGenerationMethods"].as_array();
                Some(ModelInfo {
                    id: id.to_string(),
                    supports_chat: methods.is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item.as_str() == Some("generateContent"))
                    }),
                    supports_embeddings: methods.is_some_and(|items| {
                        items
                            .iter()
                            .any(|item| item.as_str() == Some("embedContent"))
                    }),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::chat::{ReasoningEffort, Tool};

    #[test]
    fn maps_tools_and_thinking_to_gemini() {
        let req = ChatRequest {
            model: Some("gemini-test".into()),
            messages: vec![Message {
                role: Role::User,
                content: vec![ContentPart::Text { text: "hi".into() }],
                tool_calls: vec![],
                tool_call_id: None,
            }],
            tools: vec![Tool {
                name: "weather".into(),
                description: None,
                parameters: json!({"type": "object"}),
            }],
            tool_choice: Some(ToolChoice::Required),
            stream: false,
            max_tokens: Some(100),
            temperature: None,
            top_p: None,
            reasoning_effort: Some(ReasoningEffort::Low),
            thinking_budget_tokens: None,
        };
        let body = AiStudioProvider::request_body(&req);
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["name"],
            "weather"
        );
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
        assert_eq!(
            body["generationConfig"]["thinkingConfig"]["thinkingLevel"],
            "LOW"
        );
    }
}
