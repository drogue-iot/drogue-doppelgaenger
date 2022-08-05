use drogue_bazaar::runtime;
use drogue_doppelgaenger_backend::run;
use drogue_doppelgaenger_core::PROJECT;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime!(PROJECT).exec_fn(run).await
}
