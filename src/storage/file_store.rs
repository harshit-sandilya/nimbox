use anyhow::{Result, anyhow};
use directories::UserDirs;
use std::{fs, path::PathBuf};

use crate::{
    models::config::{ApiKey, Config},
    storage::store::Store,
};

pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    fn save_config(&self, config: &Config) -> Result<()> {
        let content = serde_json::to_string_pretty(config)?;

        fs::write(&self.path, content)?;

        Ok(())
    }
}

impl Store for FileStore {
    fn load_config(&self) -> Result<Config> {
        if !self.path.exists() {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(&self.path)?;

        Ok(serde_json::from_str(&content)?)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        let config = self.load_config()?;

        match key {
            "provider" => Ok(Some(config.provider)),
            "model" => Ok(config.model),
            "embedding" => Ok(config.embedding),
            _ => Err(anyhow!("unknown key")),
        }
    }

    fn get_named_keys(&self) -> Result<Vec<ApiKey>> {
        let config = self.load_config()?;

        Ok(config.keys)
    }

    fn set(&self, key: &str, value: String) -> Result<()> {
        let mut config = self.load_config()?;

        match key {
            "provider" => {
                config.provider = value;
            }
            "model" => {
                config.model = Some(value);
            }
            "embedding" => {
                config.embedding = Some(value);
            }
            _ => return Err(anyhow!("unknown key")),
        }

        self.save_config(&config)
    }

    fn add_key(&self, name: String, api_key: String) -> Result<()> {
        let mut config = self.load_config()?;

        config.keys.push(ApiKey { name, key: api_key });

        self.save_config(&config)
    }

    fn delete_key(&self, name: &str) -> Result<()> {
        let mut config = self.load_config()?;

        config.keys.retain(|k| k.name != name);

        self.save_config(&config)
    }

    fn delete_all_keys(&self) -> Result<()> {
        let mut config = self.load_config()?;

        config.keys.clear();

        self.save_config(&config)
    }
}

impl Default for FileStore {
    fn default() -> Self {
        let home = UserDirs::new()
            .expect("unable to locate home directory")
            .home_dir()
            .to_path_buf();

        let config_dir = home.join(".nimbox");

        std::fs::create_dir_all(&config_dir).expect("unable to create config directory");

        let config_path = config_dir.join("config.json");

        if !config_path.exists() {
            let config = crate::models::config::Config::default();

            let content = serde_json::to_string_pretty(&config).unwrap();

            std::fs::write(&config_path, content).unwrap();
        }

        Self { path: config_path }
    }
}
