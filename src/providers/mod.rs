pub mod nim;
pub mod provider;
use std::sync::Arc;

use crate::providers::{nim::NimProvider, provider::Provider};
use anyhow::{Result, bail};

pub fn supported_providers() -> Vec<&'static str> {
    vec![NimProvider::NAME]
}

pub fn provider_exists(name: &str) -> bool {
    supported_providers().contains(&name)
}

pub fn get_provider(name: &str) -> Result<Arc<dyn Provider>> {
    match name {
        NimProvider::NAME => Ok(Arc::new(NimProvider::new())),
        _ => bail!("unsupported provider '{}'", name),
    }
}
