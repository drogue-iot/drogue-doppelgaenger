//! Injectors allow to inject events from an external system into the internal Kafka topic

mod mapper;
pub mod mqtt;

pub use mapper::*;

use crate::{
    injector::{
        metadata::MetadataMapper,
        mqtt::{SinkTarget, Target},
        payload::PayloadMapper,
    },
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
        let target = SinkTarget {
            sink,
            metadata_mapper: self.metadata_mapper,
            payload_mapper: self.payload_mapper,
        };
        self.source.run(target).await
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceConfig {
    Mqtt(mqtt::Config),
}

impl SourceConfig {
    pub async fn run<T: Target>(self, target: T) -> anyhow::Result<()> {
        match self {
            Self::Mqtt(mqtt) => mqtt.run(target).await,
        }
    }
}
