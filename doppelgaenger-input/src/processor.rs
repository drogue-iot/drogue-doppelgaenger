use crate::{metrics, ApplicationConfig};
use bson::{spec::BinarySubtype, Bson, Document};
use chrono::Utc;
use cloudevents::{
    binding::rdkafka::MessageExt, event::ExtensionValue, AttributesReader, Data, Event,
};
use futures_util::stream::StreamExt;
use indexmap::IndexMap;
use mongodb::{
    bson::doc,
    options::{ClientOptions, UpdateOptions},
    Client, Database,
};
use rdkafka::{
    config::FromClientConfig,
    consumer::{CommitMode, Consumer, StreamConsumer},
    util::DefaultRuntime,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

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
            let event = stream.next().await.map(|r| {
                r.map_err::<anyhow::Error, _>(|err| err.into())
                    .and_then(|msg| {
                        msg.to_event()
                            .map_err(|err| err.into())
                            .map(|evt| (msg, evt))
                    })
            });

            log::debug!("Next event: {event:?}");

            match event {
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
            metrics::DELTA_T.observe((diff.num_milliseconds() as f64) / 1000.0);
        }

        if event.ty() != "io.drogue.event.v1" {
            // ignore non-data event
            log::debug!("Skipping non data event: {}", event.ty());
            return Ok(());
        }

        let twin_event = TwinEvent::try_from(event)?;

        self.process(twin_event).await?;

        Ok(())
    }

    async fn process(&self, event: TwinEvent) -> Result<(), TwinEventError> {
        log::debug!("Processing twin event: {event:?}");

        let mut set = Document::new();
        let mut unset = Document::new();

        let (application, device) = match event {
            TwinEvent::Update {
                application,
                device,
                features,
            } => {
                for (k, v) in features {
                    let v: Bson = v.try_into()?;
                    set.insert(format!("features.{k}.properties"), v);
                }
                unset.insert("payload", "");
                (application, device)
            }
            TwinEvent::Binary {
                application,
                device,
                payload,
            } => {
                set.insert(
                    "payload",
                    bson::Binary {
                        subtype: BinarySubtype::Generic,
                        bytes: payload,
                    },
                );
                unset.insert("features", "");
                (application, device)
            }
        };

        if set.is_empty() && unset.is_empty() {
            return Ok(());
        }

        let collection = self.db.collection::<ThingState>(&application);

        let update = doc! {
            "$currentDate": {
                "lastUpdateTimestamp": true,
            },
            "$inc": {
                "revision": 1,
            },
            "$set": set,
        };

        log::debug!("Request update: {:#?}", update);

        collection
            .update_one(
                doc! {
                    "device": device
                },
                update,
                Some(UpdateOptions::builder().upsert(true).build()),
            )
            .await?;

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum TwinEvent {
    Update {
        application: String,
        device: String,
        features: Map<String, Value>,
    },
    Binary {
        application: String,
        device: String,
        payload: Vec<u8>,
    },
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
        // Assume all MongoDB errors to be temporary. Might need some refinement.
        matches!(self, Self::Persistence(_))
    }
}

impl TryFrom<Event> for TwinEvent {
    type Error = TwinEventError;

    fn try_from(event: Event) -> Result<Self, Self::Error> {
        let (application, device, payload) = match (
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

        match payload {
            Payload::Json(mut payload) => {
                let features: Map<String, Value> = match payload.get_mut("features") {
                    Some(payload) => serde_json::from_value(payload.take()).map_err(|err| {
                        TwinEventError::Conversion(format!("Failed to convert: {err}"))
                    })?,
                    None => {
                        return Err(TwinEventError::Conversion(
                            "Missing 'features' field".into(),
                        ))
                    }
                };

                Ok(TwinEvent::Update {
                    application,
                    device,
                    features,
                })
            }
            Payload::Binary(payload) => Ok(TwinEvent::Binary {
                application,
                device,
                payload,
            }),
        }
    }
}

#[derive(Clone, Debug)]
enum Payload {
    Json(Value),
    Binary(Vec<u8>),
}

fn payload(mut event: Event) -> Option<Payload> {
    match event.datacontenttype() {
        Some("application/json") => match event.take_data() {
            (_, _, Some(Data::Json(json))) => Some(Payload::Json(json)),
            (_, _, Some(Data::Binary(data))) => {
                serde_json::from_slice(&data).map(Payload::Json).ok()
            }
            (_, _, Some(Data::String(data))) => serde_json::from_str(&data).map(Payload::Json).ok(),
            _ => None,
        },
        Some("application/octet-stream") => {
            // assume encrypted data, store as-is
            match event.take_data() {
                (_, _, Some(Data::Json(json))) => {
                    serde_json::to_vec(&json).map(Payload::Binary).ok()
                }
                (_, _, Some(Data::Binary(data))) => Some(Payload::Binary(data)),
                (_, _, Some(Data::String(data))) => Some(Payload::Binary(data.as_bytes().to_vec())),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod test {
    use crate::processor::{TwinEvent, TwinEventError};
    use cloudevents::{Data, EventBuilder, EventBuilderV10};
    use serde_json::{json, Value};

    #[test]
    pub fn test_invalid() {
        assert!(!parse(Data::Json(Value::Null)).unwrap_err().is_temporary());
        assert!(!parse(Data::Json(json!({}))).unwrap_err().is_temporary());
        assert!(!parse(Data::Json(json!({"foo":"bar"})))
            .unwrap_err()
            .is_temporary());
        assert!(!parse(Data::Json(json!({"features":1})))
            .unwrap_err()
            .is_temporary());
    }

    fn parse(data: Data) -> Result<TwinEvent, TwinEventError> {
        let event = EventBuilderV10::new()
            .id("id1")
            .ty("test")
            .source("test")
            .extension("application", "a1")
            .extension("device", "d1")
            .data("application/json", data)
            .build()
            .unwrap();
        let result = TwinEvent::try_from(event);
        println!("Result: {result:?}");
        result
    }
}
