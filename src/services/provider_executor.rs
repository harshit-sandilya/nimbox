use crate::app::context::AppContext;
use crate::{
    models::chat::{ChatRequest, ChatResponse, ProviderStream, StreamEvent},
    models::embedding::{EmbeddingRequest, EmbeddingResponse},
};
use anyhow::{Result, anyhow};
use futures_util::StreamExt;

pub struct ProviderExecutor;

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
    if let Some(pos) = msg.find("retry_after=") {
        msg[pos + 12..].split_whitespace().next()?.parse().ok()
    } else {
        None
    }
}

impl ProviderExecutor {
    async fn acquire_key(ctx: &AppContext) -> Result<(String, String)> {
        let mut km = ctx.key_manager.write().await;
        km.next_key().ok_or_else(|| {
            // Check if keys exist but all on cooldown vs no keys at all
            anyhow!("No API keys available — all keys may be rate limited")
        })
    }

    pub async fn chat(ctx: &AppContext, req: ChatRequest) -> Result<ChatResponse> {
        let (key_name, api_key) = Self::acquire_key(ctx).await?;
        let result = ctx.provider.chat(req, api_key).await;
        let mut km = ctx.key_manager.write().await;
        match result {
            Ok(response) => {
                km.report_success(&key_name);
                Ok(response)
            }
            Err(err) => {
                if is_rate_limit(&err) {
                    km.report_rate_limit_with_retry(&key_name, parse_retry_after(&err));
                } else {
                    km.report_error(&key_name);
                }
                Err(err)
            }
        }
    }

    pub async fn chat_stream(ctx: &AppContext, req: ChatRequest) -> Result<ProviderStream> {
        let (key_name, api_key) = Self::acquire_key(ctx).await?;
        let result = ctx.provider.chat_stream(req, api_key).await;

        match result {
            Err(err) => {
                let mut km = ctx.key_manager.write().await;
                if is_rate_limit(&err) {
                    km.report_rate_limit_with_retry(&key_name, parse_retry_after(&err));
                } else {
                    km.report_error(&key_name);
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
                                } else {
                                    km.report_error(&key_name);
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
        let (key_name, api_key) = Self::acquire_key(ctx).await?;
        let result = ctx.provider.embeddings(req, api_key).await;
        let mut km = ctx.key_manager.write().await;
        match result {
            Ok(response) => {
                km.report_success(&key_name);
                Ok(response)
            }
            Err(err) => {
                if is_rate_limit(&err) {
                    km.report_rate_limit_with_retry(&key_name, parse_retry_after(&err));
                } else {
                    km.report_error(&key_name);
                }
                Err(err)
            }
        }
    }
}
