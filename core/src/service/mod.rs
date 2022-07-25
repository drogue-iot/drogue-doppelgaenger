mod error;
mod id;
mod updater;

pub use error::*;
pub use id::Id;
pub use updater::*;

use crate::machine::{Machine, OutboxMessage, Outcome};
use crate::model::{Thing, WakerExt, WakerReason};
use crate::notifier::Notifier;
use crate::processor::sink::Sink;
use crate::processor::Event;
use crate::storage::{self, Storage};
use chrono::{Duration, Utc};
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use uuid::Uuid;

lazy_static! {
    static ref OUTBOX_EVENTS: IntCounter =
        register_int_counter!("outbox", "Number of generated outbox events").unwrap();
    static ref NOT_CHANGED: IntCounter =
        register_int_counter!("not_changed", "Number of events that didn't cause a change")
            .unwrap();
}

#[derive(Debug, serde::Deserialize)]
pub struct Config<St: Storage, No: Notifier, Si: Sink> {
    pub storage: St::Config,
    pub notifier: No::Config,
    pub sink: Si::Config,
}

impl<St: Storage, No: Notifier, Si: Sink> Clone for Config<St, No, Si> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            notifier: self.notifier.clone(),
            sink: self.sink.clone(),
        }
    }
}

pub struct Service<St: Storage, No: Notifier, Si: Sink> {
    storage: St,
    notifier: No,
    sink: Si,
}

impl<St: Storage, No: Notifier, Si: Sink> Service<St, No, Si> {
    pub fn from_config(config: Config<St, No, Si>) -> anyhow::Result<Self> {
        let Config {
            storage,
            notifier,
            sink,
        } = config;
        let storage = St::from_config(&storage)?;
        let notifier = No::from_config(&notifier)?;
        let sink = Si::from_config(sink)?;
        Ok(Self::new(storage, notifier, sink))
    }

    pub fn new(storage: St, notifier: No, sink: Si) -> Self {
        Self {
            storage,
            notifier,
            sink,
        }
    }

    pub async fn create(&self, thing: Thing) -> Result<Thing, Error<St, No>> {
        let Outcome {
            mut new_thing,
            outbox,
        } = Machine::create(thing).await?;

        let outbox = Self::add_outbox(&mut new_thing, outbox);
        OUTBOX_EVENTS.inc_by(outbox.len() as u64);

        let new_thing = self
            .storage
            .create(new_thing)
            .await
            .map_err(Error::Storage)?;

        // we can send the events right away, as we created the entry

        let new_thing = self.send_and_ack(new_thing, outbox).await?;

        // notify

        self.notifier
            .notify(&new_thing)
            .await
            .map_err(Error::Notifier)?;

        // FIXME: handle error

        log::debug!("New thing created: {new_thing:?}");

        // done

        Ok(new_thing)
    }

    pub async fn get(&self, id: &Id) -> Result<Option<Thing>, Error<St, No>> {
        self.storage
            .get(&id.application, &id.thing)
            .await
            .map_err(Error::Storage)
    }

    pub async fn delete(&self, id: &Id) -> Result<bool, Error<St, No>> {
        self.storage
            .delete(&id.application, &id.thing)
            .await
            .or_else(|err| match err {
                // if we didn't find what we want to delete, this is just fine
                storage::Error::NotFound => Ok(false),
                err => Err(Error::Storage(err)),
            })
    }

    pub async fn update<U>(&self, id: &Id, updater: U) -> Result<Thing, Error<St, No>>
    where
        U: Updater,
    {
        let current_thing = self
            .storage
            .get(&id.application, &id.thing)
            .await
            .and_then(|r| r.ok_or(storage::Error::NotFound))
            .map_err(Error::Storage)?;

        let Outcome {
            mut new_thing,
            outbox,
        } = Machine::new(current_thing.clone())
            .update(|thing| async { updater.update(thing) })
            .await?;

        let outbox = Self::add_outbox(&mut new_thing, outbox);
        OUTBOX_EVENTS.inc_by(outbox.len() as u64);

        // check diff after adding outbox events
        // TODO: maybe reconsider? if there is no state change? do we send out events? is an event a state change?
        if current_thing == new_thing {
            log::debug!("Thing state not changed. Return early!");
            NOT_CHANGED.inc();
            // no change, nothing to do
            return Ok(current_thing);
        }

        // store

        let mut new_thing = self
            .storage
            .update(new_thing)
            .await
            .map_err(Error::Storage)?;

        // send outbox events, if the outbox was empty

        // TODO: set waker to recheck for outbox events

        let current_outbox = current_thing.internal.map(|i| i.outbox.len()).unwrap_or(0);

        log::debug!("Current outbox size: {}", current_outbox);

        if current_outbox <= 0 {
            // only send when we had no previous events
            new_thing = self.send_and_ack(new_thing, outbox).await?;
        }

        // notify

        self.notifier
            .notify(&new_thing)
            .await
            .map_err(Error::Notifier)?;

        // FIXME: handle failure

        // done

        Ok(new_thing)
    }

    /// Add new, scheduled, messages to the outbox, and return the entries to send out.
    fn add_outbox(thing: &mut Thing, outbox: Vec<OutboxMessage>) -> Vec<Event> {
        if outbox.is_empty() {
            // early return
            return Vec::new();
        }

        let internal = {
            // TODO: replace with thing.internal.get_or_insert_default(); once it is stabilized
            if thing.internal.is_none() {
                thing.internal = Some(Default::default());
            }
            // unwrap is safe here, as we just set it to "some"
            thing.internal.as_mut().unwrap()
        };

        let add: Vec<_> = outbox
            .into_iter()
            .map(|message| Event {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                application: thing.metadata.application.clone(),
                device: message.thing,
                message: message.message,
            })
            .collect();

        // append events to the stored outbox

        internal.outbox.extend(add.clone());

        // schedule waker

        if !internal.outbox.is_empty() {
            internal.wakeup(Duration::seconds(30), WakerReason::Outbox);
        }

        // return the added events

        add
    }

    async fn send_and_ack(
        &self,
        mut new_thing: Thing,
        outbox: Vec<Event>,
    ) -> Result<Thing, Error<St, No>> {
        log::debug!("New outbox: {outbox:?}");

        if outbox.is_empty() {
            // early return
            return Ok(new_thing);
        }

        match self.sink.publish_iter(outbox).await {
            Ok(()) => {
                log::debug!("Outbox events sent");

                // ack outbox events
                if let Some(internal) = &mut new_thing.internal {
                    internal.outbox.clear();
                    // we can clear the waker, as we are sure that the outbox was clear initially
                    internal.clear_wakeup(WakerReason::Outbox);
                    new_thing = self
                        .storage
                        .update(new_thing)
                        .await
                        .map_err(Error::Storage)?;
                }
            }
            Err((done, err)) => {
                log::info!("Failed to send outbox events: {err:?}, done: {done}");

                // ack done events
                if let Some(internal) = &mut new_thing.internal {
                    // remove the first, done elements
                    let remaining = internal.outbox.split_off(done);
                    internal.outbox = remaining;

                    new_thing = self
                        .storage
                        .update(new_thing)
                        .await
                        .map_err(Error::Storage)?;
                }

                // FIXME: handle this case?
            }
        }

        Ok(new_thing)
    }
}
