use axum::{
    Router,
    routing::{get, post},
};

use crate::app::context::AppContext;
use crate::server::{anthropic, health, openai};

pub fn router(ctx: AppContext) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/embeddings", post(openai::embeddings))
        .route("/v1/messages", post(anthropic::messages))
        .with_state(ctx)
}
