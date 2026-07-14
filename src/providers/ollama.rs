use anyhow::{Result, anyhow};

use crate::models::chat::{ChatRequest, ChatResponse, ProviderStream};
use crate::models::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::providers::openai::OpenAIProvider;
use crate::providers::provider::{ModelInfo, Provider};

pub struct OllamaProvider {
    client: reqwest::Client,
    host: String,
    openai: OpenAIProvider,
}

impl OllamaProvider {
    pub const NAME: &'static str = "ollama";

    pub fn new() -> Self {
        let host = std::env::var("NIMBOX_OLLAMA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11434".to_string())
            .trim_end_matches('/')
            .to_string();
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("valid HTTP client"),
            openai: OpenAIProvider::compatible("Ollama", format!("{host}/v1")),
            host,
        }
    }

    fn embedding_model(req: &EmbeddingRequest) -> Result<&str> {
        req.model
            .as_deref()
            .ok_or_else(|| anyhow!("No embedding model specified"))
    }
}

#[async_trait::async_trait]
impl Provider for OllamaProvider {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn requires_api_key(&self) -> bool {
        false
    }

    async fn chat(&self, req: ChatRequest, _api_key: String) -> Result<ChatResponse> {
        self.openai.chat(req, "ollama".to_string()).await
    }

    async fn chat_stream(&self, req: ChatRequest, _api_key: String) -> Result<ProviderStream> {
        self.openai.chat_stream(req, "ollama".to_string()).await
    }

    async fn embeddings(
        &self,
        req: EmbeddingRequest,
        _api_key: String,
    ) -> Result<EmbeddingResponse> {
        let res = self
            .client
            .post(format!("{}/api/embed", self.host))
            .json(&serde_json::json!({
                "model": Self::embedding_model(&req)?,
                "input": req.input,
            }))
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            return Err(anyhow!("Ollama returned {}: {}", status, res.text().await?));
        }
        let json: serde_json::Value = res.json().await?;
        let vectors = json["embeddings"]
            .as_array()
            .ok_or_else(|| anyhow!("Invalid Ollama embedding response: missing embeddings"))?
            .iter()
            .map(|vector| {
                vector
                    .as_array()
                    .ok_or_else(|| anyhow!("Invalid Ollama embedding vector"))?
                    .iter()
                    .map(|value| {
                        value
                            .as_f64()
                            .ok_or_else(|| anyhow!("Ollama embedding contains a non-number"))
                    })
                    .collect()
            })
            .collect::<Result<Vec<Vec<f64>>>>()?;
        Ok(EmbeddingResponse { vectors })
    }

    async fn models(&self, _api_key: String) -> Result<Vec<ModelInfo>> {
        let res = self
            .client
            .get(format!("{}/api/tags", self.host))
            .send()
            .await?;
        let status = res.status();
        if !status.is_success() {
            return Err(anyhow!("Ollama returned {}: {}", status, res.text().await?));
        }
        let json: serde_json::Value = res.json().await?;
        Ok(json["models"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|model| model["model"].as_str().or_else(|| model["name"].as_str()))
            .map(|id| ModelInfo {
                id: id.to_string(),
                supports_chat: true,
                supports_embeddings: true,
            })
            .collect())
    }
}
