use crate::config::kafka::KafkaProperties;
use crate::processor::Event;
use anyhow::{anyhow, bail};
use async_trait::async_trait;
use rdkafka::config::FromClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Headers, OwnedHeaders};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::Message;
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub properties: HashMap<String, String>,

    #[serde(default)]
    pub source_properties: HashMap<String, String>,
    #[serde(default)]
    pub sink_properties: HashMap<String, String>,

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

pub struct EventStream {}

pub struct Source {
    consumer: StreamConsumer,
}

impl Source {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let topic = config.topic;

        let mut config: rdkafka::ClientConfig =
            KafkaProperties::new([config.properties, config.source_properties]).into();

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
}

#[derive(Clone)]
pub struct Sink {
    producer: FutureProducer,
    topic: String,
    timeout: Duration,
}

impl Sink {
    pub fn new(config: Config) -> anyhow::Result<Self> {
        let topic = config.topic.clone();
        let timeout = config.timeout;
        let config: rdkafka::ClientConfig =
            KafkaProperties::new([config.properties, config.sink_properties]).into();

        log::info!("Event stream - sink: {config:?}");

        let producer = FutureProducer::from_config(&config)?;

        Ok(Self {
            producer,
            topic,
            timeout,
        })
    }
}

#[async_trait]
impl super::Source for Source {
    async fn run<F, Fut>(self, mut f: F) -> anyhow::Result<()>
    where
        F: FnMut(Event) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send,
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
                            f(event).await;
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
fn extract_id(msg: &BorrowedMessage) -> anyhow::Result<(String, String)> {
    let headers = match msg.headers() {
        Some(headers) => headers,
        None => {
            bail!("Missing headers");
        }
    };

    let mut application = None;
    let mut device = None;

    for i in 0..headers.count() {
        match headers.get_as::<str>(i) {
            Some(("application", Ok(value))) => {
                application = Some(value);
            }
            Some(("device", Ok(value))) => {
                device = Some(value);
            }
            _ => {}
        }
    }

    Ok((
        application
            .ok_or_else(|| anyhow!("Missing 'application' header"))?
            .to_string(),
        device
            .ok_or_else(|| anyhow!("Missing 'device' header"))?
            .to_string(),
    ))
}

/// Parse a Kafka message into an [`Event`].
fn from_msg(msg: &BorrowedMessage) -> anyhow::Result<Event> {
    let (application, device) = extract_id(msg)?;

    let message = serde_json::from_slice(msg.payload().ok_or_else(|| anyhow!("Missing payload"))?)?;

    Ok(Event {
        application,
        device,
        message,
    })
}

#[async_trait]
impl super::Sink for Sink {
    async fn publish(&mut self, event: Event) -> anyhow::Result<()> {
        let key = format!("{}/{}", event.application, event.device);

        let payload = serde_json::to_vec(&event.message)?;

        let headers = OwnedHeaders::new()
            .add("application", &event.application)
            .add("device", &event.device);

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

impl super::EventStream for EventStream {
    type Config = Config;
    type Source = Source;
    type Sink = Sink;

    fn new(config: Self::Config) -> anyhow::Result<(Self::Source, Self::Sink)> {
        Ok((Source::new(config.clone())?, Sink::new(config)?))
    }
}
