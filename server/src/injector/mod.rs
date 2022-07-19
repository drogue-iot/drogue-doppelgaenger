//! Injectors allow to inject events from an external system into the internal Kafka topic

use drogue_doppelgaenger_core::processor::source::Sink;

mod mapper;
mod mqtt;

pub use mapper::*;

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Config {
    #[serde(default)]
    mapper: PayloadMapper,
    source: SourceConfig,
}

impl Config {
    pub async fn run<S: Sink>(self, sink: S) -> anyhow::Result<()> {
        self.source.run(sink, self.mapper).await
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceConfig {
    Mqtt(mqtt::Config),
}

impl SourceConfig {
    pub async fn run<S: Sink>(self, sink: S, mapper: PayloadMapper) -> anyhow::Result<()> {
        match self {
            Self::Mqtt(mqtt) => mqtt.run(sink, mapper).await,
        }
    }
}
