use crate::{app::context::AppContext, storage::store::Store};
use anyhow::Result;

pub fn run(ctx: &AppContext, model: String) -> Result<()> {
    ctx.store.set("embedding", model.clone())?;
    println!("Embedding model set to '{}'", model);
    Ok(())
}
