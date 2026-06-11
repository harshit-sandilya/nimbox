use crate::{
    key_manager::manager::KeyManager, providers::provider::Provider, storage::file_store::FileStore,
};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppContext {
    pub store: Arc<FileStore>,
    pub provider: Arc<dyn Provider>,
    pub key_manager: Arc<RwLock<KeyManager>>,
}
