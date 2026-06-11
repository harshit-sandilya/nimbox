use crate::{app::context::AppContext, storage::store::Store};
use anyhow::Result;

pub fn run(ctx: &AppContext, model: String) -> Result<()> {
    ctx.store.set("model", model.clone())?;
    println!("Model set to '{}'", model);
    Ok(())
}
