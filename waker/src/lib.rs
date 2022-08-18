use drogue_bazaar::app::{Startup, StartupExt};
use drogue_doppelgaenger_core::{
    processor::sink::{self},
    waker::{self, Config},
};

pub async fn run(
    config: Config<waker::postgres::Waker, sink::kafka::Sink>,
    startup: &mut dyn Startup,
) -> anyhow::Result<()> {
    let waker = waker::Processor::from_config(config)?.run();

    startup.spawn(waker);

    Ok(())
}
