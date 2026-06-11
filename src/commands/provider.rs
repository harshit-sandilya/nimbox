use crate::{
    app::context::AppContext,
    providers::{provider_exists, supported_providers},
    storage::store::Store,
};
use anyhow::{Result, bail};

pub fn run(ctx: &AppContext, provider: Option<String>, list: bool) -> Result<()> {
    if list {
        println!("Supported providers:");

        for provider in supported_providers() {
            println!("  {}", provider);
        }

        return Ok(());
    }

    let provider = provider.ok_or_else(|| anyhow::anyhow!("provider required"))?;

    if !provider_exists(&provider) {
        bail!("unsupported provider '{}'", provider);
    }

    ctx.store.set("provider", provider.clone())?;

    println!("Provider set to '{}'", provider);

    Ok(())
}
