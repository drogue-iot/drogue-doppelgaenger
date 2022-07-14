use drogue_doppelgaenger_core::app;
use drogue_doppelgaenger_processor::{run, Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app!();
}
