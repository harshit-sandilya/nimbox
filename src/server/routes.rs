use axum::{
    Router,
    routing::{get, post},
};

use crate::app::context::AppContext;
use crate::server::{anthropic, gemini, health, openai};

pub fn router(ctx: AppContext) -> Router {
    Router::new()
        .route("/health", get(health::health))
        .route("/v1/models", get(openai::models))
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/responses", post(openai::responses))
        .route("/v1/embeddings", post(openai::embeddings))
        .route("/v1/messages", post(anthropic::messages))
        .route("/v1beta/models", get(gemini::models))
        .route("/v1beta/models/{model}", get(gemini::model))
        .route("/v1beta/models/{model}", post(gemini::model_action))
        .with_state(ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key_manager::manager::KeyManager;
    use crate::providers::ollama::OllamaProvider;
    use crate::storage::file_store::FileStore;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[test]
    fn gemini_operation_routes_are_valid() {
        let ctx = AppContext {
            store: Arc::new(FileStore::at(
                std::env::temp_dir().join("nimbox-router-test.json"),
            )),
            provider: Arc::new(OllamaProvider::new()),
            key_manager: Arc::new(RwLock::new(KeyManager::new())),
        };
        let _router = router(ctx);
    }
}
