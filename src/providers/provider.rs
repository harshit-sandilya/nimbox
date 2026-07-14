use anyhow::Result;
use async_trait::async_trait;

use crate::models::chat::{ChatRequest, ChatResponse, ProviderStream};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub supports_chat: bool,
    pub supports_embeddings: bool,
}

impl ModelInfo {
    pub fn unknown(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            supports_chat: true,
            supports_embeddings: false,
        }
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;

    fn requires_api_key(&self) -> bool {
        true
    }

    async fn chat(&self, req: ChatRequest, api_key: String) -> Result<ChatResponse>;
    async fn chat_stream(&self, req: ChatRequest, api_key: String) -> Result<ProviderStream>;
    async fn embeddings(&self, req: EmbeddingRequest, api_key: String)
    -> Result<EmbeddingResponse>;

    async fn models(&self, _api_key: String) -> Result<Vec<ModelInfo>> {
        Ok(Vec::new())
    }
}
