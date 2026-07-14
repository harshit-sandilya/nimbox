use crate::app::context::AppContext;
use crate::{
    models::chat::{ChatRequest, ChatResponse, ProviderStream, StreamEvent},
    models::embedding::{EmbeddingRequest, EmbeddingResponse},
    providers::provider::ModelInfo,
};
use anyhow::{Result, anyhow};
use futures_util::StreamExt;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct ProviderExecutor;

fn key_debug_enabled() -> bool {
    matches!(
        std::env::var("NIMBOX_DEBUG_KEYS"),
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
    )
}

fn key_debug_log(message: impl AsRef<str>) {
    if key_debug_enabled() {
        eprintln!("[nimbox:key-debug] {}", message.as_ref());
    }
}

async fn key_pool_state(ctx: &AppContext) -> String {
    let km = ctx.key_manager.read().await;
    let lines = km.debug_state_lines();
    if lines.is_empty() {
        "<empty>".to_string()
    } else {
        lines.join(", ")
    }
}

// Detect rate limit from error message — adapt to match your provider error strings
fn is_rate_limit(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("429")
        || msg.contains("rate limit")
        || msg.contains("rate_limit")
        || msg.contains("too many requests")
        || msg.contains("temporarily rate-limited")
}

fn parse_retry_after(err: &anyhow::Error) -> Option<u64> {
    let msg = err.to_string();

    // Generic retry_after=NN parser
    if let Some(pos) = msg.find("retry_after=") {
        if let Some(secs) = msg[pos + 12..]
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
        {
            return Some(secs);
        }
    }

    // OpenRouter often embeds reset in JSON-like error payload:
    // "X-RateLimit-Reset":"1781395200000" (epoch millis)
    if let Some(ms) = parse_openrouter_reset_epoch_ms(&msg) {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_millis() as u64;

        if ms > now_ms {
            let diff_ms = ms - now_ms;
            // ceil(ms/1000)
            let secs = diff_ms.div_ceil(1000);
            return Some(secs.max(1));
        }
    }

    None
}

fn parse_openrouter_reset_epoch_ms(msg: &str) -> Option<u64> {
    let marker = "\"X-RateLimit-Reset\":\"";
    let start = msg.find(marker)? + marker.len();
    let tail = &msg[start..];
    let end = tail.find('"')?;
    tail[..end].parse::<u64>().ok()
}

impl ProviderExecutor {
    async fn acquire_key(ctx: &AppContext) -> Result<(String, String)> {
        if !ctx.provider.requires_api_key() {
            return Ok(("local".to_string(), String::new()));
        }
        let mut km = ctx.key_manager.write().await;
        km.next_key().ok_or_else(|| {
            anyhow!(
                "No API keys available for '{}'. Add one with: nimbox add -n default <key>",
                ctx.provider.name()
            )
        })
    }

    pub async fn models(ctx: &AppContext) -> Result<Vec<ModelInfo>> {
        let (key_name, api_key) = Self::acquire_key(ctx).await?;
        let result = ctx.provider.models(api_key).await;
        let mut km = ctx.key_manager.write().await;
        match result {
            Ok(models) => {
                km.report_success(&key_name);
                Ok(models)
            }
            Err(err) => {
                if is_rate_limit(&err) {
                    let retry_after = parse_retry_after(&err);
                    km.report_rate_limit_with_retry(&key_name, retry_after);
                } else {
                    km.report_error(&key_name);
                }
                Err(err)
            }
        }
    }

    pub async fn chat(ctx: &AppContext, req: ChatRequest) -> Result<ChatResponse> {
        let mut saw_rate_limit = false;
        let mut attempt: u32 = 0;
        let mut tried_keys: Vec<String> = Vec::new();

        loop {
            attempt += 1;
            let (key_name, api_key) = match Self::acquire_key(ctx).await {
                Ok(k) => k,
                Err(err) => {
                    if saw_rate_limit {
                        key_debug_log(format!(
                            "chat attempt={} no key available (all on cooldown)",
                            attempt
                        ));

                        let mut message =
                            "All API keys are currently rate limited. Please retry shortly."
                                .to_string();
                        if key_debug_enabled() {
                            let pool = key_pool_state(ctx).await;
                            message = format!(
                                "{} tried_keys=[{}] pool=[{}]",
                                message,
                                tried_keys.join(","),
                                pool
                            );
                        }

                        return Err(anyhow!(message));
                    }
                    key_debug_log(format!(
                        "chat attempt={} acquire_key error={}",
                        attempt, err
                    ));
                    return Err(err);
                }
            };

            key_debug_log(format!(
                "chat attempt={} key={} selected",
                attempt, key_name
            ));
            tried_keys.push(key_name.clone());

            let result = ctx.provider.chat(req.clone(), api_key).await;
            let mut km = ctx.key_manager.write().await;
            match result {
                Ok(response) => {
                    km.report_success(&key_name);
                    key_debug_log(format!("chat attempt={} key={} success", attempt, key_name));
                    return Ok(response);
                }
                Err(err) => {
                    if is_rate_limit(&err) {
                        saw_rate_limit = true;
                        let retry_after = parse_retry_after(&err);
                        km.report_rate_limit_with_retry(&key_name, retry_after);
                        key_debug_log(format!(
                            "chat attempt={} key={} rate_limited retry_after={:?} err={}",
                            attempt, key_name, retry_after, err
                        ));
                        // Try next available key in the same request.
                        continue;
                    }

                    km.report_error(&key_name);
                    key_debug_log(format!(
                        "chat attempt={} key={} provider_error err={}",
                        attempt, key_name, err
                    ));
                    return Err(err);
                }
            }
        }
    }

    pub async fn chat_stream(ctx: &AppContext, req: ChatRequest) -> Result<ProviderStream> {
        let (key_name, api_key) = Self::acquire_key(ctx).await?;
        key_debug_log(format!("chat_stream key={} selected", key_name));
        let result = ctx.provider.chat_stream(req, api_key).await;

        match result {
            Err(err) => {
                let mut km = ctx.key_manager.write().await;
                if is_rate_limit(&err) {
                    let retry_after = parse_retry_after(&err);
                    km.report_rate_limit_with_retry(&key_name, retry_after);
                    key_debug_log(format!(
                        "chat_stream key={} rate_limited retry_after={:?} err={}",
                        key_name, retry_after, err
                    ));
                } else {
                    km.report_error(&key_name);
                    key_debug_log(format!(
                        "chat_stream key={} provider_error err={}",
                        key_name, err
                    ));
                }
                Err(err)
            }
            Ok(stream) => {
                // Wrap stream — report success/failure as events arrive
                let ctx = ctx.clone();
                let wrapped = stream.map(move |event| {
                    match &event {
                        Ok(StreamEvent::Done) => {
                            let ctx = ctx.clone();
                            let key_name = key_name.clone();
                            tokio::spawn(async move {
                                ctx.key_manager.write().await.report_success(&key_name);
                                key_debug_log(format!("chat_stream key={} success", key_name));
                            });
                        }
                        Ok(StreamEvent::Error { message }) => {
                            let is_rl = message.to_lowercase().contains("rate limit")
                                || message.contains("429")
                                || message.to_lowercase().contains("temporarily rate-limited");
                            let ctx = ctx.clone();
                            let key_name = key_name.clone();
                            tokio::spawn(async move {
                                let mut km = ctx.key_manager.write().await;
                                if is_rl {
                                    km.report_rate_limit_with_retry(&key_name, None);
                                    key_debug_log(format!(
                                        "chat_stream key={} stream_rate_limited",
                                        key_name
                                    ));
                                } else {
                                    km.report_error(&key_name);
                                    key_debug_log(format!(
                                        "chat_stream key={} stream_error",
                                        key_name
                                    ));
                                }
                            });
                        }
                        _ => {}
                    }
                    event
                });
                Ok(Box::pin(wrapped))
            }
        }
    }

    pub async fn embeddings(ctx: &AppContext, req: EmbeddingRequest) -> Result<EmbeddingResponse> {
        let mut saw_rate_limit = false;
        let mut attempt: u32 = 0;
        let mut tried_keys: Vec<String> = Vec::new();

        loop {
            attempt += 1;
            let (key_name, api_key) = match Self::acquire_key(ctx).await {
                Ok(k) => k,
                Err(err) => {
                    if saw_rate_limit {
                        key_debug_log(format!(
                            "embeddings attempt={} no key available (all on cooldown)",
                            attempt
                        ));

                        let mut message =
                            "All API keys are currently rate limited. Please retry shortly."
                                .to_string();
                        if key_debug_enabled() {
                            let pool = key_pool_state(ctx).await;
                            message = format!(
                                "{} tried_keys=[{}] pool=[{}]",
                                message,
                                tried_keys.join(","),
                                pool
                            );
                        }

                        return Err(anyhow!(message));
                    }
                    key_debug_log(format!(
                        "embeddings attempt={} acquire_key error={}",
                        attempt, err
                    ));
                    return Err(err);
                }
            };

            key_debug_log(format!(
                "embeddings attempt={} key={} selected",
                attempt, key_name
            ));
            tried_keys.push(key_name.clone());

            let result = ctx.provider.embeddings(req.clone(), api_key).await;
            let mut km = ctx.key_manager.write().await;
            match result {
                Ok(response) => {
                    km.report_success(&key_name);
                    key_debug_log(format!(
                        "embeddings attempt={} key={} success",
                        attempt, key_name
                    ));
                    return Ok(response);
                }
                Err(err) => {
                    if is_rate_limit(&err) {
                        saw_rate_limit = true;
                        let retry_after = parse_retry_after(&err);
                        km.report_rate_limit_with_retry(&key_name, retry_after);
                        key_debug_log(format!(
                            "embeddings attempt={} key={} rate_limited retry_after={:?} err={}",
                            attempt, key_name, retry_after, err
                        ));
                        continue;
                    }

                    km.report_error(&key_name);
                    key_debug_log(format!(
                        "embeddings attempt={} key={} provider_error err={}",
                        attempt, key_name, err
                    ));
                    return Err(err);
                }
            }
        }
    }
}
