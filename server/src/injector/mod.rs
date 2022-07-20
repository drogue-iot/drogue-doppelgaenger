//! Injectors allow to inject events from an external system into the internal Kafka topic

use drogue_doppelgaenger_core::processor::source::Sink;

mod mapper;
mod mqtt;

use crate::injector::metadata::MetadataMapper;
use crate::injector::payload::PayloadMapper;
pub use mapper::*;

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Config {
    #[serde(default)]
    metadata_mapper: MetadataMapper,
    #[serde(default)]
    payload_mapper: PayloadMapper,
    source: SourceConfig,
}

impl Config {
    pub async fn run<S: Sink>(self, sink: S) -> anyhow::Result<()> {
        self.source
            .run(sink, self.metadata_mapper, self.payload_mapper)
            .await
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceConfig {
    Mqtt(mqtt::Config),
}

impl SourceConfig {
    pub async fn run<S: Sink>(
        self,
        sink: S,
        metadata_mapper: MetadataMapper,
        payload_mapper: PayloadMapper,
    ) -> anyhow::Result<()> {
        match self {
            Self::Mqtt(mqtt) => mqtt.run(sink, metadata_mapper, payload_mapper).await,
        }
    }
}
