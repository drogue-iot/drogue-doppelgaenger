use crate::config::kafka::KafkaProperties;
use crate::kafka::{AddHeader, KafkaHeaders};
use crate::processor::Event;
use anyhow::anyhow;
use async_trait::async_trait;
use opentelemetry::global::get_text_map_propagator;
use rdkafka::config::FromClientConfig;
use rdkafka::message::OwnedHeaders;
use rdkafka::producer::{FutureProducer, FutureRecord};
use std::{collections::HashMap, time::Duration};
use tracing::instrument;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub properties: HashMap<String, String>,

    pub topic: String,
    #[serde(with = "humantime_serde", default = "default::timeout")]
    pub timeout: Duration,
}

mod default {
    use std::time::Duration;

    pub const fn timeout() -> Duration {
        Duration::from_secs(2)
    }
}

#[derive(Clone)]
pub struct Sink {
    producer: FutureProducer,
    topic: String,
    timeout: Duration,
}

#[async_trait]
impl super::Sink for Sink {
    type Config = Config;

    fn from_config(
        Self::Config {
            properties,
            topic,
            timeout,
        }: Self::Config,
    ) -> anyhow::Result<Self> {
        let config: rdkafka::ClientConfig = KafkaProperties(properties).into();
        let producer = FutureProducer::from_config(&config)?;

        Ok(Self {
            producer,
            topic,
            timeout,
        })
    }

    #[instrument(skip_all, fields(
        id=event.id,
        timestamp=%event.timestamp,
        application=event.application,
        thing=event.thing
    ), err)]
    async fn publish(&self, event: Event) -> anyhow::Result<()> {
        let key = format!("{}/{}", event.application, event.thing);

        let payload = serde_json::to_vec(&event.message)?;

        let headers = OwnedHeaders::new()
            .add("ce_specversion", "1.0")
            .add("ce_id", &event.id)
            .add("ce_source", "drogue-doppelgaenger")
            .add("ce_type", "io.drogue.doppelgeanger.event.v1")
            .add("ce_timestamp", &event.timestamp.to_rfc3339())
            .add("content-type", "application/json")
            .add("ce_application", &event.application)
            .add("ce_thing", &event.thing);

        let mut headers = KafkaHeaders::from(headers);
        get_text_map_propagator(|prop| {
            prop.inject(&mut headers);
        });
        let headers = headers.into();

        let record = FutureRecord::to(&self.topic)
            .key(&key)
            .payload(&payload)
            .headers(headers);

        if let Err((err, _)) = self.producer.send(record, self.timeout).await {
            Err(anyhow!(err))
        } else {
            Ok(())
        }
    }
}
