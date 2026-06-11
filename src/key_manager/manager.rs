use crate::{key_manager::state::KeyState, storage::store::Store};
use anyhow::Result;
use std::time::{Duration, Instant};

pub struct KeyManager {
    keys: Vec<KeyState>,
    current: usize,
}

impl KeyManager {
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            current: 0,
        }
    }

    pub fn next_key(&mut self) -> Option<(String, String)> {
        let len = self.keys.len();
        if len == 0 {
            return None;
        }
        for _ in 0..len {
            let idx = self.current % len;
            self.current += 1;
            let key = &self.keys[idx];
            match key.cooldown_until {
                Some(until) if until > Instant::now() => continue,
                _ => return Some((key.name.clone(), key.key.clone())),
            }
        }
        None
    }

    pub fn report_rate_limit_with_retry(&mut self, key_name: &str, retry_after: Option<u64>) {
        if let Some(key) = self.keys.iter_mut().find(|k| k.name == key_name) {
            key.failures += 1;
            let secs = retry_after
                .unwrap_or_else(|| (60u64 * (1u64 << (key.failures - 1).min(6))).min(3600));
            key.cooldown_until = Some(Instant::now() + Duration::from_secs(secs));
        }
    }

    /// Call on non-rate-limit errors (network, parse, etc) — no cooldown
    pub fn report_error(&mut self, key_name: &str) {
        if let Some(key) = self.keys.iter_mut().find(|k| k.name == key_name) {
            key.failures += 1;
            // No cooldown — key still usable
        }
    }

    pub fn report_success(&mut self, key_name: &str) {
        if let Some(key) = self.keys.iter_mut().find(|k| k.name == key_name) {
            key.successes += 1;
            key.failures = 0; // Reset backoff on success
            key.cooldown_until = None;
        }
    }

    pub fn sync_with_store<S: Store>(&mut self, store: &S) -> Result<()> {
        let stored = store.get_named_keys()?;
        let stored_names: Vec<String> = stored.iter().map(|k| k.name.clone()).collect();
        for api_key in stored {
            match self.keys.iter_mut().find(|k| k.name == api_key.name) {
                Some(existing) => {
                    existing.key = api_key.key;
                }
                None => {
                    self.keys.push(KeyState {
                        name: api_key.name,
                        key: api_key.key,
                        cooldown_until: None,
                        successes: 0,
                        failures: 0,
                    });
                }
            }
        }
        self.keys
            .retain(|existing| stored_names.contains(&existing.name));
        Ok(())
    }
}
