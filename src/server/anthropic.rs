use std::time::{SystemTime, UNIX_EPOCH};

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
    ChatRequest, ContentPart, Message, ReasoningEffort, Role, StreamEvent, Tool, ToolCall,
    ToolChoice,
};
use crate::services::provider_executor::ProviderExecutor;
use crate::storage::store::Store;

pub async fn messages(
    State(ctx): State<AppContext>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    let model = match ctx.store.get("model") {
        Ok(Some(model)) => model,
        _ => {
            return anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                "No model configured. Run: nimbox model <model-name>",
            );
        }
    };

    let req = match to_internal_request(payload.clone(), model) {
        Ok(req) => req,
        Err(err) => {
            return anthropic_error_response(
                StatusCode::BAD_REQUEST,
                "invalid_request_error",
                err.to_string(),
            );
        }
    };

    if req.stream {
        let stream = match ProviderExecutor::chat_stream(&ctx, req).await {
            Ok(s) => s,
            Err(err) => {
                return anthropic_error_response(
                    status_from_error(&err),
                    "api_error",
                    err.to_string(),
                );
            }
        };

        let msg_id = format!("msg_{}", now_ms());

        let start_event = futures_util::stream::once(async move {
            Ok::<Event, axum::Error>(
                Event::default().event("message_start").data(
                    json!({
                        "type": "message_start",
                        "message": {
                            "id": msg_id,
                            "type": "message",
                            "role": "assistant",
                            "content": [],
                            "model": "proxy",
                            "stop_reason": null,
                            "stop_sequence": null,
                            "usage": {"input_tokens": 0, "output_tokens": 0}
                        }
                    })
                    .to_string(),
                ),
            )
        });

        let block_start = futures_util::stream::once(async {
            Ok::<Event, axum::Error>(
                Event::default().event("content_block_start").data(
                    json!({
                        "type": "content_block_start",
                        "index": 0,
                        "content_block": {"type": "text", "text": ""}
                    })
                    .to_string(),
                ),
            )
        });

        let mut tool_index: i64 = 0; // fix: start at 0, first tool increments to 1
        let mapped = stream.filter_map(move |event| {
            let result = anthropic_stream_event(event, &mut tool_index);
            async move { result }
        });

        let block_stop = futures_util::stream::once(async {
            Ok::<Event, axum::Error>(
                Event::default()
                    .event("content_block_stop")
                    .data(json!({"type": "content_block_stop", "index": 0}).to_string()),
            )
        });

        let message_delta = futures_util::stream::once(async {
            Ok::<Event, axum::Error>(
                Event::default().event("message_delta").data(
                    json!({
                        "type": "message_delta",
                        "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                        "usage": {"output_tokens": 0}
                    })
                    .to_string(),
                ),
            )
        });

        let stop_event = futures_util::stream::once(async {
            Ok::<Event, axum::Error>(
                Event::default()
                    .event("message_stop")
                    .data(json!({"type": "message_stop"}).to_string()),
            )
        });

        let full_stream = start_event
            .chain(block_start)
            .chain(mapped)
            .chain(block_stop)
            .chain(message_delta)
            .chain(stop_event);

        return Sse::new(full_stream).into_response();
    }

    // Non-streaming
    let response = match ProviderExecutor::chat(&ctx, req).await {
        Ok(r) => r,
        Err(err) => {
            return anthropic_error_response(status_from_error(&err), "api_error", err.to_string());
        }
    };

    Json(to_anthropic_response(response)).into_response()
}

fn anthropic_error_response(
    status: StatusCode,
    error_type: &str,
    message: impl Into<String>,
) -> axum::response::Response {
    (
        status,
        Json(json!({
            "type": "error",
            "error": {
                "type": error_type,
                "message": message.into()
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

// --- Converters ---

fn to_internal_request(payload: Value, model: String) -> anyhow::Result<ChatRequest> {
    use anyhow::anyhow;

    // Anthropic puts system as top-level string
    let mut messages: Vec<Message> = Vec::new();

    if let Some(system) = payload["system"].as_str() {
        messages.push(Message {
            role: Role::System,
            content: vec![ContentPart::Text {
                text: system.to_string(),
            }],
            tool_calls: vec![],
            tool_call_id: None,
        });
    }

    let anthropic_messages = payload["messages"]
        .as_array()
        .ok_or_else(|| anyhow!("messages missing"))?;

    for msg in anthropic_messages {
        messages.push(parse_anthropic_message(msg)?);
    }

    let tools = payload["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(parse_anthropic_tool)
                .collect::<anyhow::Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();

    let tool_choice = parse_anthropic_tool_choice(payload.get("tool_choice"));

    Ok(ChatRequest {
        model: Some(model),
        messages,
        tools,
        tool_choice,
        stream: payload["stream"].as_bool().unwrap_or(false),
        max_tokens: payload["max_tokens"].as_u64().map(|v| v as u32),
        temperature: payload["temperature"].as_f64().map(|v| v as f32),
        top_p: payload["top_p"].as_f64().map(|v| v as f32),
        reasoning_effort: parse_reasoning_effort(&payload),
        thinking_budget_tokens: payload["thinking"]["budget_tokens"]
            .as_u64()
            .map(|v| v as u32),
    })
}

fn parse_reasoning_effort(payload: &Value) -> Option<ReasoningEffort> {
    let effort = payload["reasoning_effort"]
        .as_str()
        .or_else(|| payload["reasoning"]["effort"].as_str())?;

    match effort {
        "low" => Some(ReasoningEffort::Low),
        "medium" => Some(ReasoningEffort::Medium),
        "high" => Some(ReasoningEffort::High),
        _ => None,
    }
}

fn parse_anthropic_message(value: &Value) -> anyhow::Result<Message> {
    let role = match value["role"].as_str().unwrap_or("user") {
        "assistant" => Role::Assistant,
        _ => Role::User,
    };

    let mut content: Vec<ContentPart> = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut tool_call_id: Option<String> = None;

    match &value["content"] {
        // Simple string content
        Value::String(text) => {
            content.push(ContentPart::Text { text: text.clone() });
        }
        // Array of content blocks
        Value::Array(blocks) => {
            for block in blocks {
                match block["type"].as_str().unwrap_or("") {
                    "text" => {
                        if let Some(text) = block["text"].as_str() {
                            content.push(ContentPart::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                    // Assistant tool_use block → internal ToolCall
                    "tool_use" => {
                        tool_calls.push(ToolCall {
                            id: block["id"].as_str().unwrap_or("").to_string(),
                            name: block["name"].as_str().unwrap_or("").to_string(),
                            arguments: block["input"].to_string(),
                        });
                    }
                    // User tool_result block → tool response message
                    "tool_result" => {
                        tool_call_id = block["tool_use_id"].as_str().map(ToString::to_string);
                        let result_text = match &block["content"] {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        content.push(ContentPart::Text { text: result_text });
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    // tool_result role maps to Tool
    let role = if tool_call_id.is_some() {
        Role::Tool
    } else {
        role
    };

    Ok(Message {
        role,
        content,
        tool_calls,
        tool_call_id,
    })
}

fn parse_anthropic_tool(value: &Value) -> anyhow::Result<Tool> {
    Ok(Tool {
        name: value["name"].as_str().unwrap_or("").to_string(),
        description: value["description"].as_str().map(ToString::to_string),
        // Anthropic uses "input_schema", OpenAI uses "parameters"
        parameters: value["input_schema"].clone(),
    })
}

fn parse_anthropic_tool_choice(value: Option<&Value>) -> Option<ToolChoice> {
    let value = value?;
    match value["type"].as_str()? {
        "auto" => Some(ToolChoice::Auto),
        "none" => Some(ToolChoice::None),
        "any" => Some(ToolChoice::Required),
        "tool" => Some(ToolChoice::Tool(
            value["name"].as_str().unwrap_or("").to_string(),
        )),
        _ => None,
    }
}

fn to_anthropic_response(response: crate::models::chat::ChatResponse) -> Value {
    let mut content: Vec<Value> = Vec::new();

    // Text content
    let text = response
        .message
        .content
        .iter()
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(text.as_str()),
        })
        .collect::<Vec<_>>()
        .join("");

    if !text.is_empty() {
        content.push(json!({"type": "text", "text": text}));
    }

    // Tool calls → tool_use blocks
    for call in &response.message.tool_calls {
        let input: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
        content.push(json!({
            "type": "tool_use",
            "id": call.id,
            "name": call.name,
            "input": input
        }));
    }

    let stop_reason = match response.finish_reason {
        crate::models::chat::FinishReason::ToolCalls => "tool_use",
        crate::models::chat::FinishReason::Length => "max_tokens",
        _ => "end_turn",
    };

    json!({
        "id": format!("msg_{}", now_ms()),
        "type": "message",
        "role": "assistant",
        "content": content,
        "model": "proxy",
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": response.usage.map(|u| json!({
            "input_tokens": u.prompt_tokens,
            "output_tokens": u.completion_tokens
        })).unwrap_or(json!({
            "input_tokens": 0,
            "output_tokens": 0
        }))
    })
}

fn anthropic_stream_event(
    event: anyhow::Result<StreamEvent>,
    tool_index: &mut i64,
) -> Option<Result<Event, axum::Error>> {
    let event = match event {
        Ok(e) => e,
        Err(e) => {
            return Some(Ok(Event::default().event("error").data(
                json!({
                    "type": "error",
                    "error": {"type": "api_error", "message": e.to_string()}
                })
                .to_string(),
            )));
        }
    };

    match event {
        StreamEvent::TextDelta(text) => {
            Some(Ok(Event::default().event("content_block_delta").data(
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": {"type": "text_delta", "text": text}
                })
                .to_string(),
            )))
        }

        StreamEvent::ToolCallStarted { id, name } => {
            *tool_index += 1;
            let idx = *tool_index; // +1 because index 0 = text block
            Some(Ok(Event::default().event("content_block_start").data(
                json!({
                    "type": "content_block_start",
                    "index": idx,
                    "content_block": {
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": {}
                    }
                })
                .to_string(),
            )))
        }

        StreamEvent::ToolCallDelta {
            arguments_chunk, ..
        } => {
            let idx = *tool_index;
            Some(Ok(Event::default().event("content_block_delta").data(
                json!({
                    "type": "content_block_delta",
                    "index": idx,
                    "delta": {"type": "input_json_delta", "partial_json": arguments_chunk}
                })
                .to_string(),
            )))
        }

        StreamEvent::ToolCallFinished { .. } => {
            let idx = *tool_index;
            Some(Ok(Event::default().event("content_block_stop").data(
                json!({
                    "type": "content_block_stop",
                    "index": idx
                })
                .to_string(),
            )))
        }

        StreamEvent::Done => Some(Ok(Event::default().event("message_delta").data(
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn", "stop_sequence": null},
                "usage": {"output_tokens": 0}
            })
            .to_string(),
        ))),

        StreamEvent::Usage(u) => Some(Ok(Event::default().event("message_delta").data(
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": "end_turn"},
                "usage": {"output_tokens": u.completion_tokens}
            })
            .to_string(),
        ))),

        StreamEvent::Error { message } => Some(Ok(Event::default().event("error").data(
            json!({
                "type": "error",
                "error": {"type": "api_error", "message": message}
            })
            .to_string(),
        ))),
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
