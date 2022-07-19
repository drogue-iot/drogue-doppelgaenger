pub mod source;

use crate::model::Thing;
use crate::notifier::Notifier;
use crate::processor::source::Source;
use crate::service::{Id, InfallibleUpdater, ReportedStateUpdater, Service, UpdateMode};
use crate::storage::Storage;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Event {
    pub application: String,
    pub device: String,
    pub message: Message,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum Message {
    ReportState {
        state: BTreeMap<String, Value>,
        #[serde(default)]
        partial: bool,
    },
}

pub struct Processor<S, N, I>
where
    S: Storage,
    N: Notifier,
    I: Source,
{
    service: Service<S, N>,
    source: I,
}

impl<S, N, I> Processor<S, N, I>
where
    S: Storage,
    N: Notifier,
    I: Source + Send + Sync,
{
    pub fn new(service: Service<S, N>, source: I) -> Self {
        Self { service, source }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        self.source
            .run(|event| async {
                log::debug!("Event: {event:?}");

                let Event {
                    application,
                    device,
                    message,
                } = event;
                let id = Id {
                    application,
                    thing: device,
                };

                match self.service.update(id, message).await {
                    Ok(_) => {
                        log::debug!("Processing complete ... ok!");
                    }
                    Err(err) => {
                        log::warn!("Failed to process: {err}");
                        // FIXME: need to retry, skip, or panic
                    }
                }
            })
            .await?;

        log::warn!("Event stream closed, exiting processor!");

        Ok(())
    }
}

impl InfallibleUpdater for Message {
    fn update(self, thing: Thing) -> Thing {
        match self {
            Message::ReportState { state, partial } => ReportedStateUpdater(
                state,
                match partial {
                    true => UpdateMode::Merge,
                    false => UpdateMode::Replace,
                },
            )
            .update(thing),
        }
    }
}
