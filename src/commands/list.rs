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
    let keys = ctx.store.get_named_keys()?;

    if keys.is_empty() {
        println!("No API keys configured");

        return Ok(());
    }

    println!("Configured API Keys:");

    for key in keys {
        println!("  {:15} {}", key.name, mask_key(&key.key));
    }

    Ok(())
}
