use crate::{app::context::AppContext, storage::store::Store};
use anyhow::{Result, bail};

pub async fn run(ctx: &AppContext, name: String, key: String) -> Result<()> {
    let existing = ctx.store.get_named_keys()?;

    if existing.iter().any(|k| k.name == name) {
        bail!("key '{}' already exists", name);
    }

    ctx.store.add_key(name.clone(), key)?;
    ctx.key_manager
        .write()
        .await
        .sync_with_store(ctx.store.as_ref())?;

    println!("Added key '{}'", name);

    Ok(())
}
