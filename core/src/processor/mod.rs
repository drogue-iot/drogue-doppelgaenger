pub mod sink;
pub mod source;

use crate::{
    model::{Thing, WakerReason},
    notifier::Notifier,
    processor::{sink::Sink, source::Source},
    service::{
        self, Id, JsonMergeUpdater, JsonPatchUpdater, MergeError, PatchError, ReportedStateUpdater,
        Service, UpdateMode, Updater,
    },
    storage::{self, Storage},
};
use anyhow::anyhow;
use chrono::{DateTime, Utc};
use json_patch::Patch;
use lazy_static::lazy_static;
use prometheus::{
    register_histogram, register_int_counter, register_int_counter_vec, Histogram, IntCounter,
    IntCounterVec,
};
use serde_json::Value;
use std::{collections::BTreeMap, convert::Infallible};
use uuid::Uuid;

lazy_static! {
    static ref EVENTS: IntCounter =
        register_int_counter!("events", "Number of events processed").unwrap();
    static ref UPDATES: IntCounterVec =
        register_int_counter_vec!("updates", "Event updates", &["result"]).unwrap();
    static ref PROCESSING_TIME: Histogram =
        register_histogram!("processing_time", "Time required to process events").unwrap();
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Event {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub application: String,
    pub device: String,
    pub message: Message,
}

impl Event {
    pub fn new<A: Into<String>, D: Into<String>, M: Into<Message>>(
        application: A,
        device: D,
        message: M,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            application: application.into(),
            device: device.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Message {
    ReportState {
        state: BTreeMap<String, Value>,
        #[serde(default)]
        partial: bool,
    },
    Patch(Patch),
    Merge(Value),
    Wakeup {
        reasons: Vec<WakerReason>,
    },
}

impl Message {
    pub fn report_state() -> ReportStateBuilder {
        Default::default()
    }
}

#[derive(Clone, Debug, Default)]
pub struct ReportStateBuilder {
    state: BTreeMap<String, Value>,
    partial: bool,
}

impl ReportStateBuilder {
    pub fn partial(mut self) -> Self {
        self.partial = true;
        self
    }

    pub fn full(mut self) -> Self {
        self.partial = false;
        self
    }

    pub fn state<P: Into<String>, V: Into<Value>>(mut self, property: P, value: V) -> Self {
        self.state.insert(property.into(), value.into());
        self
    }

    pub fn build(self) -> Message {
        Message::ReportState {
            state: self.state,
            partial: self.partial,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config<St: Storage, No: Notifier, Si: Sink, So: Source> {
    #[serde(bound = "")]
    pub service: service::Config<St, No, Si>,
    pub source: So::Config,
}

pub struct Processor<St, No, Si, So>
where
    St: Storage,
    No: Notifier,
    Si: Sink,
    So: Source,
{
    service: Service<St, No, Si>,
    source: So,
}

impl<St, No, Si, So> Processor<St, No, Si, So>
where
    St: Storage,
    No: Notifier,
    Si: Sink,
    So: Source,
{
    pub fn from_config(config: Config<St, No, Si, So>) -> anyhow::Result<Self> {
        let service = Service::from_config(config.service)?;
        let source = So::from_config(config.source)?;

        Ok(Self::new(service, source))
    }

    pub fn new(service: Service<St, No, Si>, source: So) -> Self {
        Self { service, source }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        self.source
            .run(|event| async {
                log::debug!("Event: {event:?}");
                EVENTS.inc();

                let _timer = PROCESSING_TIME.start_timer();

                let Event {
                    id: _,
                    timestamp: _,
                    application,
                    device,
                    message,
                } = event;
                let id = Id {
                    application,
                    thing: device,
                };

                loop {
                    match self.service.update(&id, message.clone()).await {
                        Ok(_) => {
                            log::debug!("Processing complete ... ok!");
                            UPDATES.with_label_values(&["ok"]).inc();
                            break;
                        }
                        Err(service::Error::Storage(storage::Error::PreconditionFailed)) => {
                            UPDATES.with_label_values(&["oplock"]).inc();
                            // op-lock failure, retry
                            continue;
                        }
                        Err(service::Error::Storage(storage::Error::NotFound)) => {
                            UPDATES.with_label_values(&["not-found"]).inc();
                            // the thing does not exists, skip
                            break;
                        }
                        Err(service::Error::Storage(storage::Error::NotAllowed)) => {
                            UPDATES.with_label_values(&["not-allowed"]).inc();
                            // not allowed to modify thing, skip
                            break;
                        }
                        Err(service::Error::Notifier(err)) => {
                            UPDATES.with_label_values(&["notifier"]).inc();
                            log::warn!("Failed to notify: {err}");
                            // not much we can do
                            // FIXME: consider using a circuit breaker
                            break;
                        }
                        Err(service::Error::Machine(err)) => {
                            UPDATES.with_label_values(&["machine"]).inc();
                            log::info!("Failed to process state machine: {err}");
                            // the state machine turned the state into some error (e.g. validation)
                            // ignore and continue
                            // FIXME: consider adding a "status" field with the error
                            break;
                        }
                        Err(err) => {
                            UPDATES.with_label_values(&["other"]).inc();
                            log::warn!("Failed to process: {err}");
                            return Err(anyhow!("Failed to process: {err}"));
                        }
                    }
                }

                Ok(())
            })
            .await?;

        log::warn!("Event stream closed, exiting processor!");

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MessageError {
    #[error("That should be impossible")]
    Infallible(#[from] Infallible),
    #[error("Failed to apply JSON patch: {0}")]
    Patch(#[from] PatchError),
    #[error("Failed to apply JSON merge: {0}")]
    Merge(#[from] MergeError),
}

impl Updater for Message {
    type Error = MessageError;

    fn update(self, thing: Thing) -> Result<Thing, MessageError> {
        match self {
            Message::ReportState { state, partial } => Ok(ReportedStateUpdater(
                state,
                match partial {
                    true => UpdateMode::Merge,
                    false => UpdateMode::Replace,
                },
            )
            .update(thing)?),
            Message::Patch(patch) => Ok(JsonPatchUpdater(patch).update(thing)?),
            Message::Merge(merge) => Ok(JsonMergeUpdater(merge).update(thing)?),
            Message::Wakeup { reasons } => {
                log::info!("Wakeup: {reasons:?}");

                // FIXME: we should handle the outbox reason differently

                Ok(thing)
            }
        }
    }
}
