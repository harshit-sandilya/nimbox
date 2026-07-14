use crate::{app::context::AppContext, storage::store::Store};
use anyhow::{Result, bail};

pub async fn run(ctx: &AppContext, name: String, key: String) -> Result<()> {
    let provider = ctx.store.get("provider")?.unwrap_or_default();
    let existing = ctx.store.get_provider_keys(&provider)?;

    if existing.iter().any(|k| k.name == name) {
        bail!("key '{}' already exists", name);
    }

    ctx.store.add_key(provider.clone(), name.clone(), key)?;
    ctx.key_manager
        .write()
        .await
        .sync_with_store(ctx.store.as_ref(), &provider)?;

    println!("Added key '{}' for '{}'", name, provider);

    Ok(())
}
