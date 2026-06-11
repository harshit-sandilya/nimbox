use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub model: Option<String>,
    pub embedding: Option<String>,
    pub keys: Vec<ApiKey>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "nvidia-nim".to_string(),
            model: None,
            embedding: None,
            keys: vec![],
        }
    }
}
