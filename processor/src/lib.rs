use drogue_doppelgaenger_core::processor::{
    sink::{self, Sink},
    source::{self, Source},
};
use drogue_doppelgaenger_core::{
    app::run_main,
    notifier::{self, Notifier},
    processor,
    processor::Processor,
    storage::{postgres, Storage},
};
use futures::FutureExt;

#[derive(Debug, serde::Deserialize)]
pub struct Config<St: Storage, No: Notifier, Si: Sink, So: Source> {
    // serde(bound) required as S isn't serializable: https://github.com/serde-rs/serde/issues/1296
    #[serde(bound = "")]
    pub processor: processor::Config<St, No, Si, So>,
}

pub async fn run(
    config: Config<
        postgres::Storage,
        notifier::kafka::Notifier,
        sink::kafka::Sink,
        source::kafka::Source,
    >,
) -> anyhow::Result<()> {
    let processor = Processor::from_config(config.processor)?;

    run_main([processor.run().boxed_local()]).await?;

    Ok(())
}
