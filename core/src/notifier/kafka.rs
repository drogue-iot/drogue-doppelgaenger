use super::*;
use crate::config::kafka::KafkaProperties;
use crate::model::Metadata;
use crate::notifier;
use async_trait::async_trait;
use rdkafka::{
    config::FromClientConfig,
    message::OwnedHeaders,
    producer::{FutureProducer, FutureRecord},
    util::Timeout,
};
use std::{collections::HashMap, time::Duration};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub properties: HashMap<String, String>,
    pub topic: String,
    #[serde(with = "humantime_serde", default = "default::timeout")]
    pub timeout: Duration,
}

mod default {
    use super::*;
    pub const fn timeout() -> Duration {
        Duration::from_secs(2)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Event {
    pub id: Id,
}

pub struct Notifier {
    producer: FutureProducer,
    topic: String,
    timeout: Timeout,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Kafka: {0}")]
    Kafka(#[from] rdkafka::error::KafkaError),
    #[error("Serializer: {0}")]
    Serializer(#[from] serde_json::Error),
}

impl From<Error> for notifier::Error<Error> {
    fn from(err: Error) -> Self {
        Self::Sender(err)
    }
}

#[async_trait]
impl super::Notifier for Notifier {
    type Config = Config;
    type Error = Error;

    fn new(config: &Self::Config) -> anyhow::Result<Self> {
        let topic = config.topic.clone();
        let timeout = Timeout::After(config.timeout);
        let config: rdkafka::ClientConfig = KafkaProperties(config.properties.clone()).into();
        let producer = FutureProducer::from_config(&config)?;

        Ok(Self {
            producer,
            topic,
            timeout,
        })
    }

    async fn notify(&self, thing: &Thing) -> Result<(), notifier::Error<Self::Error>> {
        let Metadata {
            application, name, ..
        } = &thing.metadata;

        log::info!("Notify change - {application} / {name}");

        let headers = OwnedHeaders::new()
            .add("application", application)
            .add("thing", name);

        let key = format!("{application}/{name}");
        let payload = serde_json::to_string(&thing).map_err(Error::Serializer)?;

        let msg = FutureRecord::<String, String>::to(&self.topic)
            .key(&key)
            .headers(headers)
            .payload(&payload);

        match self.producer.send(msg, self.timeout).await {
            Ok(r) => {
                log::info!("Notification sent: {r:?}");
                Ok(())
            }
            Err((err, _)) => Err(notifier::Error::Sender(Error::Kafka(err))),
        }
    }
}
