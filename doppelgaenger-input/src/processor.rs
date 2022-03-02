use crate::ApplicationConfig;
use bson::{Bson, Document};
use chrono::Utc;
use cloudevents::{
    binding::rdkafka::MessageExt, event::ExtensionValue, AttributesReader, Data, Event,
};
use futures_util::stream::StreamExt;
use indexmap::IndexMap;
use lazy_static::lazy_static;
use mongodb::{
    bson::doc,
    options::{ClientOptions, UpdateOptions},
    Client, Database,
};
use prometheus::{Histogram, HistogramOpts};
use rdkafka::{
    config::FromClientConfig,
    consumer::{CommitMode, Consumer, StreamConsumer},
    util::DefaultRuntime,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

lazy_static! {
    pub static ref DELTA_T: Histogram = Histogram::with_opts(HistogramOpts::new(
        "event_delta",
        "Time difference (in seconds) between ingress and processing"
    ))
    .unwrap();
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Feature {
    pub properties: IndexMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ThingState {
    pub device: String,
    pub revision: u64,
    pub features: IndexMap<String, Feature>,
}

pub struct Processor {
    consumer: StreamConsumer,
    db: Database,
}

impl Processor {
    pub async fn new(config: ApplicationConfig) -> anyhow::Result<Self> {
        // kafka

        let mut kafka_config = rdkafka::ClientConfig::new();

        kafka_config.set("bootstrap.servers", config.kafka.bootstrap_servers);
        kafka_config.extend(
            config
                .kafka
                .properties
                .into_iter()
                .map(|(k, v)| (k.replace('_', "."), v)),
        );

        let consumer = StreamConsumer::<_, DefaultRuntime>::from_config(&kafka_config)?;
        consumer.subscribe(&[&config.kafka.topic])?;

        // mongodb

        let client = {
            let options = ClientOptions::parse(&config.mongodb.url).await?;
            log::info!("MongoDB Config: {:#?}", options);
            Client::with_options(options)?
        };

        let db = client.database(&config.mongodb.database);

        Ok(Self { consumer, db })
    }

    pub async fn run(self) {
        let mut stream = self.consumer.stream();

        log::info!("Running stream...");

        loop {
            match stream.next().await.map(|r| {
                r.map_err::<anyhow::Error, _>(|err| err.into())
                    .and_then(|msg| {
                        msg.to_event()
                            .map_err(|err| err.into())
                            .map(|evt| (msg, evt))
                    })
            }) {
                None => break,
                Some(Ok(msg)) => match self.handle(msg.1).await {
                    Ok(_) => {
                        if let Err(err) = self.consumer.commit_message(&msg.0, CommitMode::Async) {
                            log::info!("Failed to ack: {err}");
                            break;
                        }
                    }
                    Err(err) if !err.is_temporary() => {
                        log::info!("Dropping event with permanent error: {}", err);
                        if let Err(err) = self.consumer.commit_message(&msg.0, CommitMode::Async) {
                            log::info!("Failed to ack: {err}");
                            break;
                        }
                    }
                    Err(err) => {
                        log::info!("Failed to handle event: {}", err);
                        break;
                    }
                },
                Some(Err(err)) => {
                    log::warn!("Failed to receive from Kafka: {err}");
                    break;
                }
            };
        }
    }

    async fn handle(&self, event: Event) -> Result<(), TwinEventError> {
        if let Some(time) = event.time() {
            let diff = Utc::now() - *time;
            DELTA_T.observe((diff.num_milliseconds() as f64) / 1000.0);
        }

        self.process(TwinEvent::try_from(event)?).await?;

        Ok(())
    }

    async fn process(&self, event: TwinEvent) -> Result<(), TwinEventError> {
        log::debug!("Processing twin event: {event:?}");

        let collection = self.db.collection::<ThingState>(&event.application);

        let mut update = Document::new();
        for (k, v) in event.features {
            let v: Bson = v.try_into()?;
            update.insert(format!("features.{k}.properties"), v);
        }

        if update.is_empty() {
            return Ok(());
        }

        update.insert(
            "lastUpdateTimestamp".to_string(),
            Bson::String("$currentDate".to_string()),
        );

        let update = doc! {
            "$setOnInsert": {
                "creationTimestamp": "$currentDate"
            },
            "$inc": {
                "revision": 1,
            },
            "$set": update,
        };

        log::debug!("Request update: {:#?}", update);

        collection
            .update_one(
                doc! {
                    "device": event.device
                },
                update,
                Some(UpdateOptions::builder().upsert(true).build()),
            )
            .await?;

        Ok(())
    }
}

#[derive(Clone, Debug)]
struct TwinEvent {
    pub application: String,
    pub device: String,
    pub features: Map<String, Value>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum TwinEventError {
    #[error("Conversion error: {0}")]
    Conversion(String),
    #[error("Persistence error: {0}")]
    Persistence(#[from] mongodb::error::Error),
    #[error("Value error: {0}")]
    Value(#[from] bson::extjson::de::Error),
}

impl TwinEventError {
    pub fn is_temporary(&self) -> bool {
        match self {
            // Assume all MongoDB errors to be temporary. Might need some refinement.
            Self::Persistence(_) => true,
            _ => false,
        }
    }
}

impl TryFrom<Event> for TwinEvent {
    type Error = TwinEventError;

    fn try_from(event: Event) -> Result<Self, Self::Error> {
        let (application, device, mut payload) = match (
            event.extension("application").cloned(),
            event.extension("device").cloned(),
            payload(event),
        ) {
            (
                Some(ExtensionValue::String(application)),
                Some(ExtensionValue::String(device)),
                Some(payload),
            ) => {
                log::debug!("Payload: {:#?}", payload);
                (application, device, payload)
            }
            _ => {
                return Err(TwinEventError::Conversion("Unknown event".into()));
            }
        };

        let features: Map<String, Value> = serde_json::from_value(payload["features"].take())
            .map_err(|err| TwinEventError::Conversion(format!("Failed to convert: {err}")))?;

        Ok(TwinEvent {
            application,
            device,
            features,
        })
    }
}

impl TwinEvent {}

fn payload(mut event: Event) -> Option<Value> {
    if event.datacontenttype() != Some("application/json") {
        return None;
    }

    match event.take_data() {
        (_, _, Some(Data::Json(json))) => Some(json),
        (_, _, Some(Data::Binary(data))) => serde_json::from_slice(&data).ok(),
        (_, _, Some(Data::String(data))) => serde_json::from_str(&data).ok(),
        _ => None,
    }
}
