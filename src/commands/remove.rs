use anyhow::{Result, bail};

use crate::{app::context::AppContext, storage::store::Store};

pub async fn run(ctx: &AppContext, name: Option<String>, all: bool) -> Result<()> {
    if all {
        ctx.store.delete_all_keys()?;
        ctx.key_manager
            .write()
            .await
            .sync_with_store(ctx.store.as_ref())?;

        println!("Removed all API keys");

        return Ok(());
    }

    let name = match name {
        Some(name) => name,
        None => bail!("provide --name or --all"),
    };

    ctx.store.delete_key(&name)?;
    ctx.key_manager
        .write()
        .await
        .sync_with_store(ctx.store.as_ref())?;

    println!("Removed key '{}'", name);

    Ok(())
}
