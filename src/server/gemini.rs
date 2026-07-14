use anyhow::{Result, anyhow};
use async_stream::stream;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{
        IntoResponse,
        sse::{Event, Sse},
    },
};
use futures_util::StreamExt;
use serde_json::{Value, json};

use crate::app::context::AppContext;
use crate::models::chat::{
    ChatRequest, ContentPart, FinishReason, Message, ReasoningEffort, Role, StreamEvent, Tool,
    ToolCall, ToolChoice,
};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::services::provider_executor::ProviderExecutor;

pub async fn model_action(
    State(ctx): State<AppContext>,
    Path(action): Path<String>,
    Json(payload): Json<Value>,
) -> axum::response::Response {
    if let Some(model) = action.strip_suffix(":generateContent") {
        return generate_content(State(ctx), Path(model.to_string()), Json(payload))
            .await
            .into_response();
    }
    if let Some(model) = action.strip_suffix(":streamGenerateContent") {
        return stream_generate_content(State(ctx), Path(model.to_string()), Json(payload))
            .await
            .into_response();
    }
    if let Some(model) = action.strip_suffix(":embedContent") {
        return embed_content(State(ctx), Path(model.to_string()), Json(payload))
            .await
            .into_response();
    }
    if let Some(model) = action.strip_suffix(":batchEmbedContents") {
        return batch_embed_contents(State(ctx), Path(model.to_string()), Json(payload))
            .await
            .into_response();
    }
    gemini_error(StatusCode::NOT_FOUND, "unknown Gemini model operation")
}

pub async fn generate_content(
    State(ctx): State<AppContext>,
    Path(model): Path<String>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let request = match to_internal_request(&model, &payload, false) {
        Ok(request) => request,
        Err(error) => return gemini_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match ProviderExecutor::chat(&ctx, request).await {
        Ok(response) => Json(to_gemini_response(response)).into_response(),
        Err(error) => gemini_error(status_from_error(&error), error.to_string()),
    }
}

pub async fn stream_generate_content(
    State(ctx): State<AppContext>,
    Path(model): Path<String>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let request = match to_internal_request(&model, &payload, true) {
        Ok(request) => request,
        Err(error) => return gemini_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    let mut provider_stream = match ProviderExecutor::chat_stream(&ctx, request).await {
        Ok(stream) => stream,
        Err(error) => return gemini_error(status_from_error(&error), error.to_string()),
    };

    let output = stream! {
        let mut tool: Option<(String, String, String)> = None;
        while let Some(event) = provider_stream.next().await {
            let value = match event {
                Ok(StreamEvent::TextDelta(text)) => Some(json!({
                    "candidates": [{
                        "content": { "role": "model", "parts": [{ "text": text }] }
                    }]
                })),
                Ok(StreamEvent::ToolCallStarted { id, name }) => {
                    tool = Some((id, name, String::new()));
                    None
                }
                Ok(StreamEvent::ToolCallDelta { arguments_chunk, .. }) => {
                    if let Some((_, _, args)) = &mut tool {
                        args.push_str(&arguments_chunk);
                    }
                    None
                }
                Ok(StreamEvent::ToolCallFinished) => tool.take().map(|(id, name, args)| {
                    json!({
                        "candidates": [{
                            "content": {
                                "role": "model",
                                "parts": [{
                                    "functionCall": {
                                        "id": id,
                                        "name": name,
                                        "args": serde_json::from_str::<Value>(&args).unwrap_or(json!({}))
                                    }
                                }]
                            }
                        }]
                    })
                }),
                Ok(StreamEvent::Usage(usage)) => Some(json!({
                    "usageMetadata": {
                        "promptTokenCount": usage.prompt_tokens,
                        "candidatesTokenCount": usage.completion_tokens,
                        "totalTokenCount": usage.total_tokens
                    }
                })),
                Ok(StreamEvent::Done) => Some(json!({
                    "candidates": [{ "finishReason": "STOP" }]
                })),
                Ok(StreamEvent::Error { message }) => Some(json!({
                    "error": { "code": 502, "message": message, "status": "UNKNOWN" }
                })),
                Err(error) => Some(json!({
                    "error": { "code": 502, "message": error.to_string(), "status": "UNKNOWN" }
                })),
            };
            if let Some(value) = value {
                yield Ok::<Event, axum::Error>(Event::default().data(value.to_string()));
            }
        }
    };
    Sse::new(output).into_response()
}

pub async fn embed_content(
    State(ctx): State<AppContext>,
    Path(model): Path<String>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let text = match content_text(&payload["content"]) {
        Some(text) => text,
        None => return gemini_error(StatusCode::BAD_REQUEST, "content.parts must contain text"),
    };
    match ProviderExecutor::embeddings(
        &ctx,
        EmbeddingRequest {
            input: vec![text],
            model: Some(normalize_model(&model)),
        },
    )
    .await
    {
        Ok(response) => Json(json!({
            "embedding": { "values": response.vectors.into_iter().next().unwrap_or_default() }
        }))
        .into_response(),
        Err(error) => gemini_error(status_from_error(&error), error.to_string()),
    }
}

pub async fn batch_embed_contents(
    State(ctx): State<AppContext>,
    Path(model): Path<String>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let inputs = match payload["requests"].as_array() {
        Some(requests) => requests
            .iter()
            .map(|request| {
                content_text(&request["content"])
                    .ok_or_else(|| anyhow!("every request.content must contain text"))
            })
            .collect::<Result<Vec<_>>>(),
        None => Err(anyhow!("requests missing")),
    };
    let inputs = match inputs {
        Ok(inputs) => inputs,
        Err(error) => return gemini_error(StatusCode::BAD_REQUEST, error.to_string()),
    };
    match ProviderExecutor::embeddings(
        &ctx,
        EmbeddingRequest {
            input: inputs,
            model: Some(normalize_model(&model)),
        },
    )
    .await
    {
        Ok(response) => Json(batch_embedding_response(response)).into_response(),
        Err(error) => gemini_error(status_from_error(&error), error.to_string()),
    }
}

pub async fn models(State(ctx): State<AppContext>) -> impl IntoResponse {
    match ProviderExecutor::models(&ctx).await {
        Ok(models) => Json(json!({
            "models": models.into_iter().map(|model| {
                let mut methods = Vec::new();
                if model.supports_chat { methods.push("generateContent"); }
                if model.supports_embeddings { methods.push("embedContent"); }
                json!({
                    "name": format!("models/{}", model.id),
                    "baseModelId": model.id,
                    "displayName": model.id,
                    "supportedGenerationMethods": methods
                })
            }).collect::<Vec<_>>()
        }))
        .into_response(),
        Err(error) => gemini_error(status_from_error(&error), error.to_string()),
    }
}

pub async fn model(
    State(ctx): State<AppContext>,
    Path(requested): Path<String>,
) -> impl IntoResponse {
    let requested = normalize_model(&requested);
    match ProviderExecutor::models(&ctx).await {
        Ok(models) => match models.into_iter().find(|model| model.id == requested) {
            Some(model) => {
                let mut methods = Vec::new();
                if model.supports_chat {
                    methods.push("generateContent");
                }
                if model.supports_embeddings {
                    methods.push("embedContent");
                }
                Json(json!({
                    "name": format!("models/{}", model.id),
                    "baseModelId": model.id,
                    "displayName": model.id,
                    "supportedGenerationMethods": methods
                }))
                .into_response()
            }
            None => gemini_error(
                StatusCode::NOT_FOUND,
                format!("model '{requested}' not found"),
            ),
        },
        Err(error) => gemini_error(status_from_error(&error), error.to_string()),
    }
}

fn normalize_model(model: &str) -> String {
    model.strip_prefix("models/").unwrap_or(model).to_string()
}

fn content_text(content: &Value) -> Option<String> {
    let text = content["parts"]
        .as_array()?
        .iter()
        .filter_map(|part| part["text"].as_str())
        .collect::<String>();
    (!text.is_empty()).then_some(text)
}

fn to_internal_request(model: &str, payload: &Value, stream: bool) -> Result<ChatRequest> {
    let mut messages = Vec::new();
    if let Some(system) = content_text(&payload["systemInstruction"])
        .or_else(|| content_text(&payload["system_instruction"]))
    {
        messages.push(Message {
            role: Role::System,
            content: vec![ContentPart::Text { text: system }],
            tool_calls: vec![],
            tool_call_id: None,
        });
    }

    let mut calls_by_name = std::collections::HashMap::<String, String>::new();
    for content in payload["contents"]
        .as_array()
        .ok_or_else(|| anyhow!("contents missing"))?
    {
        let mut message = Message {
            role: if content["role"].as_str() == Some("model") {
                Role::Assistant
            } else {
                Role::User
            },
            content: vec![],
            tool_calls: vec![],
            tool_call_id: None,
        };
        for (index, part) in content["parts"]
            .as_array()
            .ok_or_else(|| anyhow!("content.parts missing"))?
            .iter()
            .enumerate()
        {
            if let Some(text) = part["text"].as_str() {
                message
                    .content
                    .push(ContentPart::Text { text: text.into() });
            }
            if let Some(call) = part
                .get("functionCall")
                .or_else(|| part.get("function_call"))
            {
                let id = call["id"]
                    .as_str()
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("call_{}_{}", messages.len(), index));
                let name = call["name"].as_str().unwrap_or("").to_string();
                calls_by_name.insert(name.clone(), id.clone());
                message.tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments: call["args"].to_string(),
                });
            }
            if let Some(response) = part
                .get("functionResponse")
                .or_else(|| part.get("function_response"))
            {
                let name = response["name"].as_str().unwrap_or("tool");
                message.role = Role::Tool;
                message.tool_call_id = response["id"]
                    .as_str()
                    .map(ToString::to_string)
                    .or_else(|| calls_by_name.get(name).cloned());
                message.content.push(ContentPart::Text {
                    text: response["response"].to_string(),
                });
            }
        }
        messages.push(message);
    }

    let tools_value = payload["tools"].as_array();
    let tools = tools_value
        .into_iter()
        .flatten()
        .flat_map(|tool| {
            tool["functionDeclarations"]
                .as_array()
                .or_else(|| tool["function_declarations"].as_array())
                .into_iter()
                .flatten()
        })
        .map(|function| Tool {
            name: function["name"].as_str().unwrap_or("").to_string(),
            description: function["description"].as_str().map(ToString::to_string),
            parameters: function["parameters"].clone(),
        })
        .collect();

    let function_config = payload["toolConfig"]["functionCallingConfig"]
        .as_object()
        .map(|_| &payload["toolConfig"]["functionCallingConfig"])
        .or_else(|| {
            payload["tool_config"]["function_calling_config"]
                .as_object()
                .map(|_| &payload["tool_config"]["function_calling_config"])
        });
    let tool_choice = function_config.map(|config| {
        let allowed = config["allowedFunctionNames"]
            .as_array()
            .or_else(|| config["allowed_function_names"].as_array());
        if let Some(name) = allowed
            .and_then(|names| names.first())
            .and_then(Value::as_str)
        {
            return ToolChoice::Tool(name.to_string());
        }
        match config["mode"].as_str().unwrap_or("AUTO") {
            "NONE" => ToolChoice::None,
            "ANY" | "VALIDATED" => ToolChoice::Required,
            _ => ToolChoice::Auto,
        }
    });

    let generation = &payload["generationConfig"];
    let thinking = &generation["thinkingConfig"];
    let reasoning_effort = match thinking["thinkingLevel"].as_str() {
        Some("LOW") => Some(ReasoningEffort::Low),
        Some("MEDIUM") => Some(ReasoningEffort::Medium),
        Some("HIGH") => Some(ReasoningEffort::High),
        _ => None,
    };
    Ok(ChatRequest {
        model: Some(normalize_model(model)),
        messages,
        tools,
        tool_choice,
        stream,
        max_tokens: generation["maxOutputTokens"]
            .as_u64()
            .map(|value| value as u32),
        temperature: generation["temperature"].as_f64().map(|value| value as f32),
        top_p: generation["topP"].as_f64().map(|value| value as f32),
        reasoning_effort,
        thinking_budget_tokens: thinking["thinkingBudget"]
            .as_u64()
            .map(|value| value as u32),
    })
}

fn to_gemini_response(response: crate::models::chat::ChatResponse) -> Value {
    let mut parts = response
        .message
        .content
        .iter()
        .map(|part| match part {
            ContentPart::Text { text } => json!({ "text": text }),
        })
        .collect::<Vec<_>>();
    parts.extend(response.message.tool_calls.iter().map(|call| {
        json!({
            "functionCall": {
                "id": call.id,
                "name": call.name,
                "args": serde_json::from_str::<Value>(&call.arguments).unwrap_or(json!({}))
            }
        })
    }));
    json!({
        "candidates": [{
            "content": { "role": "model", "parts": parts },
            "finishReason": match response.finish_reason {
                FinishReason::Length => "MAX_TOKENS",
                FinishReason::ContentFilter => "SAFETY",
                FinishReason::Error => "OTHER",
                _ => "STOP"
            }
        }],
        "usageMetadata": response.usage.map(|usage| json!({
            "promptTokenCount": usage.prompt_tokens,
            "candidatesTokenCount": usage.completion_tokens,
            "totalTokenCount": usage.total_tokens
        }))
    })
}

fn batch_embedding_response(response: EmbeddingResponse) -> Value {
    json!({
        "embeddings": response.vectors.into_iter().map(|values| json!({ "values": values })).collect::<Vec<_>>()
    })
}

fn gemini_error(status: StatusCode, message: impl Into<String>) -> axum::response::Response {
    (
        status,
        Json(json!({
            "error": {
                "code": status.as_u16(),
                "message": message.into(),
                "status": status.canonical_reason().unwrap_or("UNKNOWN").replace(' ', "_").to_uppercase()
            }
        })),
    )
        .into_response()
}

fn status_from_error(error: &anyhow::Error) -> StatusCode {
    let message = error.to_string().to_lowercase();
    if message.contains("429") || message.contains("rate limit") {
        StatusCode::TOO_MANY_REQUESTS
    } else if message.contains("401") || message.contains("unauthorized") {
        StatusCode::UNAUTHORIZED
    } else if message.contains("403") || message.contains("forbidden") {
        StatusCode::FORBIDDEN
    } else if message.contains("404") || message.contains("not found") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_GATEWAY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gemini_function_call() {
        let request = to_internal_request(
            "gemini-test",
            &json!({
                "contents": [{
                    "role": "model",
                    "parts": [{ "functionCall": { "name": "weather", "args": { "city": "Pune" } } }]
                }]
            }),
            false,
        )
        .unwrap();
        assert_eq!(request.messages[0].tool_calls[0].name, "weather");
        assert_eq!(request.model.as_deref(), Some("gemini-test"));
    }
}
