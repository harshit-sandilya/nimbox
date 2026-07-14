use anyhow::Result;

use crate::models::config::{ApiKey, Config};

pub trait Store {
    fn load_config(&self) -> Result<Config>;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn get_provider_keys(&self, provider: &str) -> Result<Vec<ApiKey>>;
    fn set(&self, key: &str, value: String) -> Result<()>;
    fn add_key(&self, provider: String, name: String, api_key: String) -> Result<()>;
    fn delete_key(&self, provider: &str, name: &str) -> Result<()>;
    fn delete_all_keys(&self, provider: &str) -> Result<()>;
}
