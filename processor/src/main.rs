use drogue_bazaar::runtime;
use drogue_doppelgaenger_core::PROJECT;
use drogue_doppelgaenger_processor::run;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime!(PROJECT).exec_fn(run).await
}
