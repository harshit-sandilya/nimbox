use anyhow::{Result, bail};

use crate::{app::context::AppContext, storage::store::Store};

pub async fn run(ctx: &AppContext, name: Option<String>, all: bool) -> Result<()> {
    let provider = ctx.store.get("provider")?.unwrap_or_default();

    if all {
        ctx.store.delete_all_keys(&provider)?;
        ctx.key_manager
            .write()
            .await
            .sync_with_store(ctx.store.as_ref(), &provider)?;

        println!("Removed all API keys for '{}'", provider);

        return Ok(());
    }

    let name = match name {
        Some(name) => name,
        None => bail!("provide --name or --all"),
    };

    ctx.store.delete_key(&provider, &name)?;
    ctx.key_manager
        .write()
        .await
        .sync_with_store(ctx.store.as_ref(), &provider)?;

    println!("Removed key '{}'", name);

    Ok(())
}
