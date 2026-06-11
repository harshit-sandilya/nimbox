use anyhow::Result;

use crate::models::config::{ApiKey, Config};

pub trait Store {
    fn load_config(&self) -> Result<Config>;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn get_named_keys(&self) -> Result<Vec<ApiKey>>;
    fn set(&self, key: &str, value: String) -> Result<()>;
    fn add_key(&self, name: String, api_key: String) -> Result<()>;
    fn delete_key(&self, name: &str) -> Result<()>;
    fn delete_all_keys(&self) -> Result<()>;
}
