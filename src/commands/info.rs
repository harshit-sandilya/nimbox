use anyhow::Result;

use crate::{app::context::AppContext, storage::store::Store};

pub fn run(ctx: &AppContext) -> Result<()> {
    let provider = ctx.store.get("provider")?.unwrap_or_default();

    let model = ctx.store.get("model")?.unwrap_or_else(|| "not set".into());

    let embedding = ctx
        .store
        .get("embedding")?
        .unwrap_or_else(|| "not set".into());

    let keys = ctx.store.get_named_keys()?;

    println!("Provider:");
    println!("  {}", provider);

    println!();

    println!("Chat Model:");
    println!("  {}", model);

    println!();

    println!("Embedding Model:");
    println!("  {}", embedding);

    println!();

    println!("API Keys:");
    println!("  {}", keys.len());

    for key in keys {
        println!("    - {}", key.name);
    }

    Ok(())
}
