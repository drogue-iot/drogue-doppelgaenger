use drogue_doppelgaenger_backend::{run, Config};
use drogue_doppelgaenger_core::app;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app!();
}
