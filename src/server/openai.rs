use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use axum::{
    Json,
    extract::State,
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
    ChatRequest, ContentPart, FinishReason, Message, Role, StreamEvent, Tool, ToolCall, ToolChoice,
};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::services::provider_executor::ProviderExecutor;
use crate::storage::store::Store;

pub async fn chat_completions(
    State(ctx): State<AppContext>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let model = match ctx.store.get("model") {
        Ok(Some(model)) => model,
        _ => {
            return openai_error_response(
                StatusCode::BAD_REQUEST,
                "No model configured. Run: nimbox model <model-name>",
            );
        }
    };

    let req = match to_internal_chat_request(payload.clone(), model) {
        Ok(req) => req,
        Err(err) => {
            return openai_error_response(StatusCode::BAD_REQUEST, err.to_string());
        }
    };

    if req.stream {
        let stream = match ProviderExecutor::chat_stream(&ctx, req).await {
            Ok(stream) => stream,
            Err(err) => {
                return openai_error_response(status_from_error(&err), err.to_string());
            }
        };

        let sse_stream = stream.map(openai_stream_event);

        return Sse::new(sse_stream).into_response();
    }

    let response = match ProviderExecutor::chat(&ctx, req).await {
        Ok(response) => response,
        Err(err) => {
            return openai_error_response(status_from_error(&err), err.to_string());
        }
    };

    Json(to_openai_chat_response(response)).into_response()
}

pub async fn embeddings(
    State(ctx): State<AppContext>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let model = match ctx.store.get("embedding") {
        Ok(Some(model)) => model,
        _ => {
            return openai_error_response(
                StatusCode::BAD_REQUEST,
                "No embedding model configured. Run: nimbox embed <model-name>",
            );
        }
    };

    let req = match to_internal_embedding_request(payload, model.clone()) {
        Ok(req) => req,
        Err(err) => {
            return openai_error_response(StatusCode::BAD_REQUEST, err.to_string());
        }
    };

    let response = match ProviderExecutor::embeddings(&ctx, req).await {
        Ok(response) => response,
        Err(err) => {
            return openai_error_response(status_from_error(&err), err.to_string());
        }
    };

    Json(to_openai_embedding_response(model, response)).into_response()
}

fn openai_error_response(
    status: StatusCode,
    message: impl Into<String>,
) -> axum::response::Response {
    (
        status,
        Json(json!({
            "error": {
                "message": message.into(),
                "type": "api_error"
            }
        })),
    )
        .into_response()
}

fn status_from_error(err: &anyhow::Error) -> StatusCode {
    let msg = err.to_string().to_lowercase();
    if msg.contains("429") || msg.contains("rate limit") || msg.contains("too many requests") {
        StatusCode::TOO_MANY_REQUESTS
    } else if msg.contains("401") || msg.contains("unauthorized") {
        StatusCode::UNAUTHORIZED
    } else if msg.contains("403") || msg.contains("forbidden") {
        StatusCode::FORBIDDEN
    } else if msg.contains("404") || msg.contains("not found") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_GATEWAY
    }
}

fn to_internal_chat_request(payload: Value, model: String) -> anyhow::Result<ChatRequest> {
    let messages = payload["messages"]
        .as_array()
        .ok_or_else(|| anyhow!("messages missing"))?
        .iter()
        .map(parse_openai_message)
        .collect::<anyhow::Result<Vec<_>>>()?;

    let tools = payload["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(parse_openai_tool)
                .collect::<anyhow::Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    let tool_choice = parse_tool_choice(payload.get("tool_choice"));

    Ok(ChatRequest {
        model: Some(model),
        messages,
        tools,
        tool_choice,
        stream: payload["stream"].as_bool().unwrap_or(false),
        max_tokens: payload["max_tokens"].as_u64().map(|v| v as u32),
        temperature: payload["temperature"].as_f64().map(|v| v as f32),
        top_p: payload["top_p"].as_f64().map(|v| v as f32),
    })
}

fn to_internal_embedding_request(
    payload: Value,
    model: String,
) -> anyhow::Result<EmbeddingRequest> {
    let input = match &payload["input"] {
        Value::String(text) => {
            vec![text.clone()]
        }
        Value::Array(arr) => arr
            .iter()
            .map(|v| {
                v.as_str()
                    .ok_or_else(|| anyhow!("input array must contain strings"))
                    .map(String::from)
            })
            .collect::<anyhow::Result<Vec<String>>>()?,
        _ => {
            anyhow::bail!("input must be string or array of strings");
        }
    };
    Ok(EmbeddingRequest {
        input,
        model: Some(model),
    })
}

fn parse_openai_message(value: &Value) -> anyhow::Result<Message> {
    let role = match value["role"].as_str().unwrap_or("user") {
        "system" => Role::System,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    };

    let content = match value.get("content") {
        Some(Value::String(text)) => {
            vec![ContentPart::Text { text: text.clone() }]
        }
        _ => vec![],
    };

    let tool_calls = value["tool_calls"]
        .as_array()
        .map(|calls| {
            calls
                .iter()
                .map(|call| ToolCall {
                    id: call["id"].as_str().unwrap_or("").to_string(),
                    name: call["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: call["function"]["arguments"]
                        .as_str()
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Message {
        role,
        content,
        tool_calls,
        tool_call_id: value["tool_call_id"].as_str().map(ToString::to_string),
    })
}

fn parse_openai_tool(value: &Value) -> anyhow::Result<Tool> {
    Ok(Tool {
        name: value["function"]["name"].as_str().unwrap_or("").to_string(),
        description: value["function"]["description"]
            .as_str()
            .map(ToString::to_string),
        parameters: value["function"]["parameters"].clone(),
    })
}

fn parse_tool_choice(value: Option<&Value>) -> Option<ToolChoice> {
    let value = value?;

    match value {
        Value::String(s) => match s.as_str() {
            "auto" => Some(ToolChoice::Auto),
            "none" => Some(ToolChoice::None),
            "required" => Some(ToolChoice::Required),
            _ => None,
        },

        Value::Object(_) => Some(ToolChoice::Tool(
            value["function"]["name"].as_str().unwrap_or("").to_string(),
        )),

        _ => None,
    }
}

fn to_openai_chat_response(response: crate::models::chat::ChatResponse) -> Value {
    json!({
        "id": format!("chatcmpl-{}", now_ms()),
        "object": "chat.completion",
        "created": now_secs(),
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": extract_text(&response.message),
                "tool_calls": response.message.tool_calls.iter().map(|call| {
                    json!({
                        "id": call.id,
                        "type": "function",
                        "function": {
                            "name": call.name,
                            "arguments": call.arguments
                        }
                    })
                }).collect::<Vec<_>>()
            },
            "finish_reason": finish_reason_to_openai(
                &response.finish_reason
            )
        }],
        "usage": response.usage.map(|u| {
            json!({
                "prompt_tokens": u.prompt_tokens,
                "completion_tokens": u.completion_tokens,
                "total_tokens": u.total_tokens
            })
        })
    })
}

fn to_openai_embedding_response(model: String, response: EmbeddingResponse) -> Value {
    json!({
        "object": "list",
        "model": model,
        "data": response.vectors.iter().enumerate().map(|(idx, vec)| {
            json!({
                "object": "embedding",
                "index": idx,
                "embedding": vec
            })
        }).collect::<Vec<_>>()
    })
}

fn openai_stream_event(event: anyhow::Result<StreamEvent>) -> Result<Event, axum::Error> {
    let event = match event {
        Ok(e) => e,
        Err(e) => {
            let data = json!({
                "error": {
                    "message": e.to_string(),
                    "type": "api_error"
                }
            });
            return Ok(Event::default().data(data.to_string()));
        }
    };

    let data = match event {
        StreamEvent::TextDelta(text) => json!({
            "choices": [{
                "delta": {
                    "content": text
                }
            }]
        }),

        StreamEvent::ToolCallStarted { id, name } => json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": id,
                        "type": "function",
                        "function": {
                            "name": name
                        }
                    }]
                }
            }]
        }),

        StreamEvent::ToolCallDelta {
            id,
            arguments_chunk,
        } => json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "id": id,
                        "function": {
                            "arguments": arguments_chunk
                        }
                    }]
                }
            }]
        }),

        StreamEvent::Done => {
            return Ok(Event::default().data("[DONE]"));
        }

        StreamEvent::Error { message } => json!({
            "error": {
                "message": message,
                "type": "api_error"
            }
        }),

        StreamEvent::Usage(_) | StreamEvent::ToolCallFinished { .. } => json!({}),
    };

    Ok(Event::default().data(data.to_string()))
}

fn extract_text(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text } => Some(text.clone()),
        })
        .collect::<Vec<_>>()
        .join("")
}

fn finish_reason_to_openai(reason: &FinishReason) -> &'static str {
    match reason {
        FinishReason::Stop => "stop",
        FinishReason::Length => "length",
        FinishReason::ToolCalls => "tool_calls",
        FinishReason::ContentFilter => "content_filter",
        FinishReason::Error => "error",
        FinishReason::Unknown => "stop",
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
