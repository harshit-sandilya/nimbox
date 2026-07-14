use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub name: String,
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub model: Option<String>,
    pub embedding: Option<String>,
    #[serde(default)]
    pub models: HashMap<String, String>,
    #[serde(default)]
    pub embeddings: HashMap<String, String>,
    pub keys: Vec<ApiKey>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "ollama".to_string(),
            model: None,
            embedding: None,
            models: HashMap::new(),
            embeddings: HashMap::new(),
            keys: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_legacy_config_without_provider_scopes() {
        let config: Config = serde_json::from_str(
            r#"{
                "provider": "nvidia-nim",
                "model": "chat-model",
                "embedding": "embed-model",
                "keys": [{"name": "default", "key": "secret"}]
            }"#,
        )
        .unwrap();

        assert!(config.models.is_empty());
        assert_eq!(config.keys[0].provider, None);
    }

    #[test]
    fn new_install_defaults_to_keyless_ollama() {
        assert_eq!(Config::default().provider, "ollama");
    }
}
