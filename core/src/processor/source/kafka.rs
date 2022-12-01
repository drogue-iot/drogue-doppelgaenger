use crate::config::kafka::KafkaProperties;
use crate::processor::Event;
use anyhow::{anyhow, bail};
use async_trait::async_trait;
use rdkafka::{
    config::FromClientConfig,
    consumer::{Consumer, StreamConsumer},
    message::{BorrowedMessage, Headers},
    Message,
};
use std::str::from_utf8;
use std::{collections::HashMap, future::Future};
use tracing::instrument;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub properties: HashMap<String, String>,

    pub topic: String,
}

pub struct EventStream {}

pub struct Source {
    consumer: StreamConsumer,
}

impl Source {
    #[instrument(skip_all, fields(
        id = event.id,
        application = event.application,
        thing = event.thing,
    ))]
    async fn process<F, Fut>(f: &F, event: Event) -> anyhow::Result<()>
    where
        F: Fn(Event) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send,
    {
        f(event).await
    }
}

#[async_trait]
impl super::Source for Source {
    type Config = Config;

    fn from_config(config: Self::Config) -> anyhow::Result<Self> {
        let topic = config.topic;

        let mut config: rdkafka::ClientConfig = KafkaProperties(config.properties).into();

        config.set("enable.partition.eof", "false");

        // configure for QoS 1

        config.set("enable.auto.commit", "true");
        config.set("auto.commit.interval.ms", "5000");
        config.set("enable.auto.offset.store", "false");

        // log config result

        log::info!("Event stream - source: {config:?}");

        let consumer = StreamConsumer::from_config(&config)?;
        consumer.subscribe(&[&topic])?;

        Ok(Self { consumer })
    }

    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: Fn(Event) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send,
    {
        log::info!("Running event source loop...");

        let consumer = self.consumer;

        loop {
            let msg = consumer.recv().await;

            match msg {
                Ok(msg) => {
                    match from_msg(&msg) {
                        Ok(event) => {
                            log::debug!("Processing event: {event:?}");

                            if let Err(err) = Self::process(&f, event).await {
                                log::error!("Handler failed: {err}");
                                break;
                            }
                        }
                        Err(err) => {
                            log::info!("Unable to parse message, skipping! Reason: {err}");
                            // we still store the offset, as we are skipping the message.
                        }
                    }
                    if let Err(err) = consumer.store_offset_from_message(&msg) {
                        log::warn!("Failed to store offset: {err}");
                        break;
                    }
                }
                Err(err) => {
                    log::warn!("Failed to receive from Kafka: {err}");
                    break;
                }
            }
        }

        log::warn!("Exiting consumer loop");

        Ok(())
    }
}

/// Extract the ID (application, device) from the message.
fn extract_meta(msg: &BorrowedMessage) -> anyhow::Result<(String, String, String, String)> {
    let headers = match msg.headers() {
        Some(headers) => headers,
        None => {
            bail!("Missing headers");
        }
    };

    let mut id = None;
    let mut timestamp = None;
    let mut application = None;
    let mut thing = None;

    for h in headers.iter() {
        match h.key {
            "ce_id" => {
                id = h.value.and_then(|s| from_utf8(s).ok());
            }
            "ce_timestamp" => {
                timestamp = h.value.and_then(|s| from_utf8(s).ok());
            }
            "ce_application" => {
                application = h.value.and_then(|s| from_utf8(s).ok());
            }
            "ce_thing" => {
                thing = h.value.and_then(|s| from_utf8(s).ok());
            }
            _ => {}
        }
    }

    Ok((
        id.ok_or_else(|| anyhow!("Missing 'ce_id' header"))?
            .to_string(),
        timestamp
            .ok_or_else(|| anyhow!("Missing 'ce_timestamp' header"))?
            .to_string(),
        application
            .ok_or_else(|| anyhow!("Missing 'ce_application' header"))?
            .to_string(),
        thing
            .ok_or_else(|| anyhow!("Missing 'ce_thing' header"))?
            .to_string(),
    ))
}

/// Parse a Kafka message into an [`Event`].
fn from_msg(msg: &BorrowedMessage) -> anyhow::Result<Event> {
    let (id, timestamp, application, thing) = extract_meta(msg)?;

    let message = serde_json::from_slice(msg.payload().ok_or_else(|| anyhow!("Missing payload"))?)?;

    Ok(Event {
        id,
        timestamp: timestamp.parse()?,
        application,
        thing,
        message,
    })
}
