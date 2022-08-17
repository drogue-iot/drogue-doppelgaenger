use drogue_bazaar::app::{Startup, StartupExt};
use drogue_doppelgaenger_core::{
    injector,
    processor::sink::{kafka, Sink},
};

#[derive(Debug, serde::Deserialize)]
pub struct Config<Si>
where
    Si: Sink,
{
    pub injector: injector::Config,
    pub sink: Si::Config,
}

pub async fn run(config: Config<kafka::Sink>, startup: &mut dyn Startup) -> anyhow::Result<()> {
    let sink = kafka::Sink::from_config(config.sink)?;
    startup.spawn(config.injector.run(sink));

    Ok(())
}
