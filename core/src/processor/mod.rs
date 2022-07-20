pub mod source;

use crate::model::Thing;
use crate::notifier::Notifier;
use crate::processor::source::{Sink, Source};
use crate::service::JsonMergeUpdater;
use crate::{
    service::{
        self, Id, JsonPatchUpdater, MergeError, PatchError, ReportedStateUpdater, Service,
        UpdateMode, Updater,
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
use std::collections::BTreeMap;
use std::convert::Infallible;

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
}

pub struct Processor<S, N, I, Si>
where
    S: Storage,
    N: Notifier,
    I: Source,
    Si: Sink,
{
    service: Service<S, N, Si>,
    source: I,
}

impl<S, N, I, Si> Processor<S, N, I, Si>
where
    S: Storage,
    N: Notifier,
    I: Source,
    Si: Sink,
{
    pub fn new(service: Service<S, N, Si>, source: I) -> Self {
        Self { service, source }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        self.source
            .run(|event| async {
                log::debug!("Event: {event:?}");
                EVENTS.inc();

                let _ = PROCESSING_TIME.start_timer();

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
        }
    }
}
