use drogue_bazaar::app::{Startup, StartupExt};
use drogue_doppelgaenger_core::{
    command::{mqtt, CommandSink},
    notifier::{self, Notifier},
    processor::{
        self,
        sink::{self, Sink},
        source::{self, Source},
        Processor,
    },
    storage::{postgres, Storage},
};

#[derive(Debug, serde::Deserialize)]
pub struct Config<St: Storage, No: Notifier, Si: Sink, So: Source, Cmd: CommandSink> {
    // serde(bound) required as S isn't serializable: https://github.com/serde-rs/serde/issues/1296
    #[serde(bound = "")]
    pub processor: processor::Config<St, No, Si, So, Cmd>,
}

pub async fn run(
    config: Config<
        postgres::Storage,
        notifier::kafka::Notifier,
        sink::kafka::Sink,
        source::kafka::Source,
        mqtt::CommandSink,
    >,
    startup: &mut dyn Startup,
) -> anyhow::Result<()> {
    let processor = Processor::from_config(startup, config.processor)?;

    startup.spawn(processor.run());

    Ok(())
}
