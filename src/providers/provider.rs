use anyhow::Result;
use async_trait::async_trait;

use crate::models::chat::{ChatRequest, ChatResponse, ProviderStream};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, req: ChatRequest, api_key: String) -> Result<ChatResponse>;
    async fn chat_stream(&self, req: ChatRequest, api_key: String) -> Result<ProviderStream>;
    async fn embeddings(&self, req: EmbeddingRequest, api_key: String)
    -> Result<EmbeddingResponse>;
}
