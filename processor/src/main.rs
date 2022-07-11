use drogue_doppelgaenger_common::app;
use drogue_doppelgaenger_processor::{run, Config};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app!();
}
