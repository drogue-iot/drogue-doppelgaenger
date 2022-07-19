use drogue_doppelgaenger_core::{
    app::run_main,
    notifier::{self, Notifier},
    processor::{
        source::{kafka, EventStream},
        Processor,
    },
    service::{self, Service},
    storage::{postgres, Storage},
};
use futures::FutureExt;

#[derive(Debug, serde::Deserialize)]
pub struct Config<S: Storage, N: Notifier> {
    // serde(bound) required as S isn't serializable: https://github.com/serde-rs/serde/issues/1296
    #[serde(bound = "")]
    pub service: service::Config<S, N>,

    pub kafka: kafka::Config,
}

pub async fn run(
    config: Config<postgres::Storage, notifier::kafka::Notifier>,
) -> anyhow::Result<()> {
    let service = Service::new(config.service)?;
    let (source, sink) = kafka::EventStream::new(config.kafka)?;
    let processor = Processor::new(service, source);

    run_main([processor.run().boxed_local()]).await?;

    Ok(())
}
