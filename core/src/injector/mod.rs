//! Injectors allow to inject events from an external system into the internal Kafka topic

mod mapper;
mod mqtt;

pub use mapper::*;

use crate::{
    injector::{metadata::MetadataMapper, payload::PayloadMapper},
    processor::sink::Sink,
};

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Config {
    /// allow to disable running the injector
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub metadata_mapper: MetadataMapper,
    #[serde(default)]
    pub payload_mapper: PayloadMapper,
    pub source: SourceConfig,
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
