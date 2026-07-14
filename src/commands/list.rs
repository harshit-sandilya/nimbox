use anyhow::Result;

use crate::{app::context::AppContext, storage::store::Store};

fn mask_key(key: &str) -> String {
    if key.len() < 8 {
        return "*****".to_string();
    }

    let suffix = &key[key.len() - 3..];

    format!("*****{}", suffix)
}

pub fn run(ctx: &AppContext) -> Result<()> {
    let provider = ctx.store.get("provider")?.unwrap_or_default();
    let keys = ctx.store.get_provider_keys(&provider)?;

    if keys.is_empty() {
        println!("No API keys configured for '{}'", provider);

        return Ok(());
    }

    println!("Configured API keys for '{}':", provider);

    for key in keys {
        println!("  {:15} {}", key.name, mask_key(&key.key));
    }

    Ok(())
}
