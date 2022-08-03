pub mod sink;
pub mod source;

use crate::{
    app::Spawner,
    command::CommandSink,
    model::{Reconciliation, Thing, WakerReason},
    notifier::Notifier,
    processor::{sink::Sink, source::Source},
    service::{
        self, Cleanup, DesiredStateValueUpdater, Id, InfallibleUpdater, JsonMergeUpdater,
        JsonPatchUpdater, MapValueInserter, MapValueRemover, ReportedStateUpdater, Service,
        UpdateMode, UpdateOptions, Updater, UpdaterExt,
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
    pub thing: String,
    pub message: Message,
}

impl Event {
    pub fn new<A: Into<String>, T: Into<String>, M: Into<Message>>(
        application: A,
        thing: T,
        message: M,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            application: application.into(),
            thing: thing.into(),
            message: message.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum SetDesiredValue {
    #[serde(rename_all = "camelCase")]
    WithOptions {
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        valid_until: Option<DateTime<Utc>>,
    },
    Value(Value),
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Message {
    ReportState {
        state: BTreeMap<String, Value>,
        #[serde(default)]
        partial: bool,
    },
    SetDesiredValue {
        #[serde(default)]
        values: BTreeMap<String, SetDesiredValue>,
    },
    Patch(Patch),
    Merge(Value),
    Wakeup {
        reasons: Vec<WakerReason>,
    },
    /// Create a thing if it doesn't yet exists, and register a child.
    RegisterChild {
        #[serde(rename = "$ref")]
        r#ref: String,
        #[serde(default)]
        template: ThingTemplate,
    },
    /// Unregister a child, and delete the thing if it was the last child.
    UnregisterChild {
        #[serde(rename = "$ref")]
        r#ref: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThingTemplate {
    #[serde(default, skip_serializing_if = "Reconciliation::is_empty")]
    pub reconciliation: Reconciliation,
}

impl InfallibleUpdater for ThingTemplate {
    fn update(&self, mut thing: Thing) -> Thing {
        thing
            .reconciliation
            .changed
            .extend(self.reconciliation.changed.clone());
        thing
            .reconciliation
            .timers
            .extend(self.reconciliation.timers.clone());
        thing
            .reconciliation
            .deleting
            .extend(self.reconciliation.deleting.clone());

        thing
    }
}

impl Message {
    pub fn report_state(partial: bool) -> ReportStateBuilder {
        ReportStateBuilder::new(partial)
    }
}

#[derive(Clone, Debug)]
pub struct ReportStateBuilder {
    state: BTreeMap<String, Value>,
    partial: bool,
}

impl ReportStateBuilder {
    pub fn new(partial: bool) -> Self {
        Self {
            state: Default::default(),
            partial,
        }
    }

    pub fn partial() -> Self {
        Self {
            state: Default::default(),
            partial: true,
        }
    }

    pub fn full() -> Self {
        Self {
            state: Default::default(),
            partial: false,
        }
    }

    pub fn state<P: Into<String>, V: Into<Value>>(mut self, property: P, value: V) -> Self {
        self.state.insert(property.into(), value.into());
        self
    }
}

impl From<ReportStateBuilder> for Message {
    fn from(builder: ReportStateBuilder) -> Self {
        Message::ReportState {
            state: builder.state,
            partial: builder.partial,
        }
    }
}

impl From<ReportStateBuilder> for ReportedStateUpdater {
    fn from(builder: ReportStateBuilder) -> Self {
        ReportedStateUpdater(builder.state, UpdateMode::from_partial(builder.partial))
    }
}

impl InfallibleUpdater for ReportStateBuilder {
    fn update(&self, thing: Thing) -> Thing {
        InfallibleUpdater::update(&ReportedStateUpdater::from(self.clone()), thing)
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config<St: Storage, No: Notifier, Si: Sink, So: Source, Cmd: CommandSink> {
    #[serde(bound = "")]
    pub service: service::Config<St, No, Si, Cmd>,
    pub source: So::Config,
}

pub struct Processor<St, No, Si, So, Cmd>
where
    St: Storage,
    No: Notifier,
    Si: Sink,
    So: Source,
    Cmd: CommandSink,
{
    service: Service<St, No, Si, Cmd>,
    source: So,
}

impl<St, No, Si, So, Cmd> Processor<St, No, Si, So, Cmd>
where
    St: Storage,
    No: Notifier,
    Si: Sink,
    So: Source,
    Cmd: CommandSink,
{
    pub fn from_config(
        spawner: &mut dyn Spawner,
        config: Config<St, No, Si, So, Cmd>,
    ) -> anyhow::Result<Self> {
        let service = Service::from_config(spawner, config.service)?;
        let source = So::from_config(config.source)?;

        Ok(Self::new(service, source))
    }

    pub fn new(service: Service<St, No, Si, Cmd>, source: So) -> Self {
        Self { service, source }
    }

    /// Cleanup a thing, ignore if missing.
    ///
    /// NOTE: This function respects a change in the `deletion_timestamp` and will trigger a
    /// deletion if the updater sets it.
    async fn run_cleanup<U>(
        service: &Service<St, No, Si, Cmd>,
        id: &Id,
        updater: U,
    ) -> Result<(), anyhow::Error>
    where
        U: Updater + Send + Sync,
    {
        let opts = UpdateOptions {
            ignore_unclean_inbox: false,
        };

        loop {
            let thing = service.get(&id).await?;
            match thing {
                Some(thing) => {
                    if thing.metadata.deletion_timestamp.is_some() {
                        log::debug!("Thing is already being deleted");
                        // cleaned up
                        break;
                    }
                    let thing = updater.update(thing)?;

                    let result = if thing.metadata.deletion_timestamp.is_some() {
                        // perform delete
                        service
                            .delete(&id, Some(&(&thing).into()))
                            .await
                            .map(|_| ())
                    } else {
                        // perform update
                        service.update(&id, &thing, &opts).await.map(|_| ())
                    };
                    match result {
                        Ok(_) => {
                            break;
                        }
                        Err(service::Error::Storage(storage::Error::PreconditionFailed)) => {
                            // retry
                            continue;
                        }
                        Err(service::Error::Storage(storage::Error::NotFound)) => {
                            // ok, we clean up anyway
                            break;
                        }
                        Err(err) => {
                            return Err(anyhow!(err));
                        }
                    }
                }
                None => {
                    // cleaned up
                    break;
                }
            }
        }

        Ok(())
    }

    /// Either update or insert a new thing
    async fn run_upsert<U>(
        service: &Service<St, No, Si, Cmd>,
        id: &Id,
        updater: U,
    ) -> Result<(), anyhow::Error>
    where
        U: Updater + Send + Sync,
    {
        let opts = UpdateOptions {
            ignore_unclean_inbox: false,
        };

        // FIXME: consider taking this into the service

        loop {
            let thing = service.get(&id).await?;
            match thing {
                Some(thing) => {
                    let thing = updater.update(thing)?;
                    match service.update(&id, &thing, &opts).await {
                        Ok(_) => {
                            break;
                        }
                        Err(service::Error::Storage(
                            storage::Error::NotFound | storage::Error::PreconditionFailed,
                        )) => {
                            // retry
                            continue;
                        }
                        Err(err) => {
                            return Err(anyhow!(err));
                        }
                    }
                }
                None => {
                    let thing = Thing::new(&id.application, &id.thing);
                    let thing = updater.update(thing)?;

                    match service.create(thing).await {
                        Ok(_) => {
                            break;
                        }
                        Err(service::Error::Storage(storage::Error::AlreadyExists)) => {
                            // retry
                            continue;
                        }
                        Err(err) => {
                            return Err(anyhow!(err));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn run_update<U>(
        service: &Service<St, No, Si, Cmd>,
        id: &Id,
        updater: U,
    ) -> Result<(), anyhow::Error>
    where
        U: Updater,
    {
        let opts = UpdateOptions {
            ignore_unclean_inbox: false,
        };

        loop {
            match service.update(id, &updater, &opts).await {
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
                    log::info!("Thing not found: {id}");
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
                    thing,
                    message,
                } = event;
                let id = Id { application, thing };

                match message {
                    Message::RegisterChild { r#ref, template } => {
                        Self::run_upsert(
                            &self.service,
                            &id,
                            MapValueInserter("$children".to_string(), r#ref).and_then(template),
                        )
                        .await?;
                    }
                    Message::UnregisterChild { r#ref } => {
                        Self::run_cleanup(
                            &self.service,
                            &id,
                            MapValueRemover("$children".to_string(), r#ref)
                                .and_then(Cleanup("$children".to_string())),
                        )
                        .await?;
                    }
                    Message::ReportState { state, partial } => {
                        Self::run_update(
                            &self.service,
                            &id,
                            ReportedStateUpdater(
                                state,
                                match partial {
                                    true => UpdateMode::Merge,
                                    false => UpdateMode::Replace,
                                },
                            ),
                        )
                        .await?
                    }
                    Message::Merge(merge) => {
                        Self::run_update(&self.service, &id, JsonMergeUpdater(merge)).await?
                    }
                    Message::Patch(patch) => {
                        Self::run_update(&self.service, &id, JsonPatchUpdater(patch)).await?
                    }
                    Message::Wakeup { reasons: _ } => {
                        // don't do any real change, this will just reconcile and process what is necessary
                        Self::run_update(&self.service, &id, ()).await?
                    }
                    Message::SetDesiredValue { values } => {
                        Self::run_update(&self.service, &id, DesiredStateValueUpdater(values))
                            .await?
                    }
                }

                Ok(())
            })
            .await?;

        log::warn!("Event stream closed, exiting processor!");

        Ok(())
    }
}
