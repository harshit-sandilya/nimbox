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
    #[cfg(test)]
    pub(crate) fn at(path: PathBuf) -> Self {
        Self { path }
    }

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
            "model" => Ok(config.models.get(&config.provider).cloned().or_else(|| {
                config
                    .models
                    .is_empty()
                    .then(|| config.model.clone())
                    .flatten()
            })),
            "embedding" => Ok(config
                .embeddings
                .get(&config.provider)
                .cloned()
                .or_else(|| {
                    config
                        .embeddings
                        .is_empty()
                        .then(|| config.embedding.clone())
                        .flatten()
                })),
            _ => Err(anyhow!("unknown key")),
        }
    }

    fn get_provider_keys(&self, provider: &str) -> Result<Vec<ApiKey>> {
        let config = self.load_config()?;

        Ok(config
            .keys
            .into_iter()
            .filter(|key| key.provider.as_deref().is_none_or(|p| p == provider))
            .collect())
    }

    fn set(&self, key: &str, value: String) -> Result<()> {
        let mut config = self.load_config()?;

        match key {
            "provider" => {
                let previous = config.provider.clone();
                if let Some(model) = config.model.take() {
                    config.models.entry(previous.clone()).or_insert(model);
                }
                if let Some(embedding) = config.embedding.take() {
                    config
                        .embeddings
                        .entry(previous.clone())
                        .or_insert(embedding);
                }
                for key in &mut config.keys {
                    if key.provider.is_none() {
                        key.provider = Some(previous.clone());
                    }
                }
                config.provider = value;
            }
            "model" => {
                config.models.insert(config.provider.clone(), value);
            }
            "embedding" => {
                config.embeddings.insert(config.provider.clone(), value);
            }
            _ => return Err(anyhow!("unknown key")),
        }

        self.save_config(&config)
    }

    fn add_key(&self, provider: String, name: String, api_key: String) -> Result<()> {
        let mut config = self.load_config()?;

        config.keys.push(ApiKey {
            name,
            key: api_key,
            provider: Some(provider),
        });

        self.save_config(&config)
    }

    fn delete_key(&self, provider: &str, name: &str) -> Result<()> {
        let mut config = self.load_config()?;

        config
            .keys
            .retain(|k| k.name != name || k.provider.as_deref().is_some_and(|p| p != provider));

        self.save_config(&config)
    }

    fn delete_all_keys(&self, provider: &str) -> Result<()> {
        let mut config = self.load_config()?;

        config
            .keys
            .retain(|k| k.provider.as_deref().is_some_and(|p| p != provider));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switching_provider_migrates_legacy_models_and_keys() {
        let path = std::env::temp_dir().join(format!(
            "nimbox-config-test-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("migration")
        ));
        std::fs::write(
            &path,
            r#"{
                "provider": "nvidia-nim",
                "model": "nim-chat",
                "embedding": "nim-embed",
                "keys": [{"name": "default", "key": "secret"}]
            }"#,
        )
        .unwrap();
        let store = FileStore { path: path.clone() };

        store.set("provider", "ollama".into()).unwrap();
        let config = store.load_config().unwrap();

        assert_eq!(config.models.get("nvidia-nim").unwrap(), "nim-chat");
        assert_eq!(config.embeddings.get("nvidia-nim").unwrap(), "nim-embed");
        assert_eq!(config.keys[0].provider.as_deref(), Some("nvidia-nim"));
        assert_eq!(store.get("model").unwrap(), None);
        std::fs::remove_file(path).unwrap();
    }
}
