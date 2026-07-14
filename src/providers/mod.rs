pub mod ai_studio;
pub mod groq;
pub mod nim;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod provider;
use std::sync::Arc;

use crate::providers::{
    ai_studio::AiStudioProvider, groq::GroqProvider, nim::NimProvider, ollama::OllamaProvider,
    openai::OpenAIProvider, openrouter::OpenRouterProvider, provider::Provider,
};
use anyhow::{Result, bail};

pub fn supported_providers() -> Vec<&'static str> {
    vec![
        OllamaProvider::NAME,
        GroqProvider::NAME,
        AiStudioProvider::NAME,
        NimProvider::NAME,
        OpenRouterProvider::NAME,
        OpenAIProvider::NAME,
    ]
}

pub fn provider_exists(name: &str) -> bool {
    canonical_provider(name).is_some()
}

pub fn canonical_provider(name: &str) -> Option<&'static str> {
    match name {
        OllamaProvider::NAME => Some(OllamaProvider::NAME),
        GroqProvider::NAME => Some(GroqProvider::NAME),
        AiStudioProvider::NAME | "gemini" | "google-ai-studio" => Some(AiStudioProvider::NAME),
        NimProvider::NAME => Some(NimProvider::NAME),
        OpenRouterProvider::NAME => Some(OpenRouterProvider::NAME),
        OpenAIProvider::NAME => Some(OpenAIProvider::NAME),
        _ => None,
    }
}

pub fn get_provider(name: &str) -> Result<Arc<dyn Provider>> {
    match canonical_provider(name) {
        Some(OllamaProvider::NAME) => Ok(Arc::new(OllamaProvider::new())),
        Some(GroqProvider::NAME) => Ok(Arc::new(GroqProvider::new())),
        Some(AiStudioProvider::NAME) => Ok(Arc::new(AiStudioProvider::new())),
        Some(NimProvider::NAME) => Ok(Arc::new(NimProvider::new())),
        Some(OpenRouterProvider::NAME) => Ok(Arc::new(OpenRouterProvider::new())),
        Some(OpenAIProvider::NAME) => Ok(Arc::new(OpenAIProvider::new())),
        _ => bail!("unsupported provider '{}'", name),
    }
}
