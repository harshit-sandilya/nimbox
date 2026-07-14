use anyhow::{Result, anyhow};

use crate::models::chat::{ChatRequest, ChatResponse, ProviderStream};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::providers::openai::OpenAIProvider;
use crate::providers::provider::{ModelInfo, Provider};

pub struct GroqProvider {
    inner: OpenAIProvider,
}

impl GroqProvider {
    pub const NAME: &'static str = "groq";

    pub fn new() -> Self {
        let base_url = std::env::var("NIMBOX_GROQ_URL")
            .unwrap_or_else(|_| "https://api.groq.com/openai/v1".to_string());
        Self {
            inner: OpenAIProvider::compatible("Groq", base_url),
        }
    }
}

#[async_trait::async_trait]
impl Provider for GroqProvider {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    async fn chat(&self, req: ChatRequest, api_key: String) -> Result<ChatResponse> {
        self.inner.chat(req, api_key).await
    }

    async fn chat_stream(&self, req: ChatRequest, api_key: String) -> Result<ProviderStream> {
        self.inner.chat_stream(req, api_key).await
    }

    async fn embeddings(
        &self,
        _req: EmbeddingRequest,
        _api_key: String,
    ) -> Result<EmbeddingResponse> {
        Err(anyhow!("Groq does not provide a text embeddings endpoint"))
    }

    async fn models(&self, api_key: String) -> Result<Vec<ModelInfo>> {
        self.inner.models(api_key).await
    }
}
