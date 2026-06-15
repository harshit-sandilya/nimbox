pub mod nim;
pub mod openai;
pub mod openrouter;
pub mod provider;
use std::sync::Arc;

use crate::providers::{
    nim::NimProvider, openai::OpenAIProvider, openrouter::OpenRouterProvider, provider::Provider,
};
use anyhow::{Result, bail};

pub fn supported_providers() -> Vec<&'static str> {
    vec![
        NimProvider::NAME,
        OpenRouterProvider::NAME,
        OpenAIProvider::NAME,
    ]
}

pub fn provider_exists(name: &str) -> bool {
    supported_providers().contains(&name)
}

pub fn get_provider(name: &str) -> Result<Arc<dyn Provider>> {
    match name {
        NimProvider::NAME => Ok(Arc::new(NimProvider::new())),
        OpenRouterProvider::NAME => Ok(Arc::new(OpenRouterProvider::new())),
        OpenAIProvider::NAME => Ok(Arc::new(OpenAIProvider::new())),
        _ => bail!("unsupported provider '{}'", name),
    }
}
