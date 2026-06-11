mod app;
mod cli;
mod commands;
mod key_manager;
mod models;
mod providers;
mod server;
mod services;
mod storage;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use std::sync::Arc;
use tokio::sync::RwLock;

use app::context::AppContext;
use storage::file_store::FileStore;
use storage::store::Store;

use crate::key_manager::manager::KeyManager;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Arc::new(FileStore::default());

    let provider_name = store
        .get("provider")?
        .unwrap_or_else(|| "nvidia-nim".to_string());

    let provider = providers::get_provider(&provider_name)?;
    let key_manager = {
        let mut km = KeyManager::new();
        km.sync_with_store(store.as_ref())?;
        Arc::new(RwLock::new(km))
    };

    let ctx = AppContext {
        store,
        provider,
        key_manager,
    };

    match cli.command {
        Commands::Start { port } => commands::start::run(&ctx, port).await,
        Commands::Stop => commands::stop::run(&ctx),
        Commands::List => commands::list::run(&ctx),
        Commands::Add { name, key } => commands::add::run(&ctx, name, key).await,
        Commands::Remove { name, all } => commands::remove::run(&ctx, name, all).await,
        Commands::Model { model } => commands::model::run(&ctx, model),
        Commands::Embed { model } => commands::embed::run(&ctx, model),
        Commands::Provider { provider, list } => commands::provider::run(&ctx, provider, list),
        Commands::Info => commands::info::run(&ctx),
        Commands::Update => commands::update::run().await,
    }?;

    Ok(())
}
