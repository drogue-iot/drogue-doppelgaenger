mod error;
mod id;
mod updater;

use chrono::Utc;
pub use error::*;
pub use id::Id;
use std::convert::Infallible;
pub use updater::*;
use uuid::Uuid;

use crate::machine::{Machine, OutboxMessage, Outcome};
use crate::model::Thing;
use crate::notifier::Notifier;
use crate::processor::source::Sink;
use crate::processor::Event;
use crate::storage::{self, Storage};
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};

lazy_static! {
    static ref OUTBOX_EVENTS: IntCounter =
        register_int_counter!("outbox", "Number of generated outbox events").unwrap();
}

#[derive(Debug, serde::Deserialize)]
pub struct Config<S: Storage, N: Notifier> {
    pub storage: S::Config,
    pub notifier: N::Config,
}

impl<S: Storage, N: Notifier> Clone for Config<S, N> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            notifier: self.notifier.clone(),
        }
    }
}

pub struct Service<S: Storage, N: Notifier, Si: Sink> {
    storage: S,
    notifier: N,
    sink: Si,
}

pub trait Updater {
    type Error: std::error::Error + 'static;

    fn update(self, thing: Thing) -> Result<Thing, Self::Error>;
}

pub trait InfallibleUpdater {
    fn update(self, thing: Thing) -> Thing;
}

impl<I> Updater for I
where
    I: InfallibleUpdater,
{
    type Error = Infallible;

    fn update(self, thing: Thing) -> Result<Thing, Self::Error> {
        Ok(InfallibleUpdater::update(self, thing))
    }
}

impl<S: Storage, N: Notifier, Si: Sink> Service<S, N, Si> {
    // FIXME: I don't like having the sink provided here directly. Need to split this up!
    pub fn new(config: Config<S, N>, sink: Si) -> anyhow::Result<Self> {
        let Config { storage, notifier } = config;
        let storage = S::new(&storage)?;
        let notifier = N::new(&notifier)?;
        Ok(Self {
            storage,
            notifier,
            sink,
        })
    }

    pub async fn create(&self, thing: Thing) -> Result<(), Error<S, N>> {
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

        self.send_and_ack(new_thing, outbox).await?;

        // FIXME: handle error

        Ok(())
    }

    pub async fn get(&self, id: Id) -> Result<Thing, Error<S, N>> {
        self.storage
            .get(&id.application, &id.thing)
            .await
            .map_err(Error::Storage)
    }

    pub async fn delete(&self, id: Id) -> Result<(), Error<S, N>> {
        self.storage
            .delete(&id.application, &id.thing)
            .await
            .or_else(|err| match err {
                // if we didn't find what we want to delete, this is just fine
                storage::Error::NotFound => Ok(()),
                err => Err(Error::Storage(err)),
            })
    }

    pub async fn update<U>(&self, id: &Id, updater: U) -> Result<(), Error<S, N>>
    where
        U: Updater,
    {
        let current_thing = self
            .storage
            .get(&id.application, &id.thing)
            .await
            .map_err(Error::Storage)?;

        let Outcome {
            mut new_thing,
            outbox,
        } = Machine::new(current_thing.clone())
            .update(|thing| async { updater.update(thing) })
            .await?;

        let outbox = Self::add_outbox(&mut new_thing, outbox);

        // check diff after adding outbox events
        // TODO: maybe reconsider? if there is no state change? do we send out events? is an event a state change?
        if current_thing == new_thing {
            log::debug!("Thing state not changed. Return early!");
            // no change, nothing to do
            return Ok(());
        }

        OUTBOX_EVENTS.inc_by(outbox.len() as u64);

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
            // only sending when we had no previous events
            new_thing = self.send_and_ack(new_thing, outbox).await?;
        }

        // notify

        self.notifier
            .notify(&new_thing)
            .await
            .map_err(Error::Notifier)?;

        // FIXME: handle failure

        // done

        Ok(())
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

        // return the added events

        add
    }

    async fn send_and_ack(
        &self,
        mut new_thing: Thing,
        outbox: Vec<Event>,
    ) -> Result<Thing, Error<S, N>> {
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
