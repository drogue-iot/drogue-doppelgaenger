use drogue_doppelgaenger_core::{
    app::{run_main, Spawner},
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
use futures::FutureExt;

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
) -> anyhow::Result<()> {
    let mut spawner = vec![];
    let processor = Processor::from_config(&mut spawner, config.processor)?;

    spawner.spawn(processor.run().boxed_local());

    run_main(spawner).await?;

    Ok(())
}
