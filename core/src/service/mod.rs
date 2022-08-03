mod error;
mod id;
mod updater;

pub use error::*;
pub use id::Id;
pub use updater::*;

use crate::app::Spawner;
use crate::command::CommandSink;
use crate::machine::{DeletionOutcome, Machine, OutboxMessage, Outcome};
use crate::model::{Thing, WakerExt, WakerReason};
use crate::notifier::Notifier;
use crate::processor::{sink::Sink, Event};
use crate::storage::{self, Storage};
use crate::Preconditions;
use chrono::{Duration, Utc};
use lazy_static::lazy_static;
use prometheus::{register_int_counter, IntCounter};
use uuid::Uuid;

lazy_static! {
    static ref OUTBOX_EVENTS: IntCounter =
        register_int_counter!("outbox", "Number of generated outbox events").unwrap();
    static ref COMMANDS: IntCounter =
        register_int_counter!("commands", "Number of generated commands").unwrap();
    static ref NOT_CHANGED: IntCounter =
        register_int_counter!("not_changed", "Number of events that didn't cause a change")
            .unwrap();
}

#[derive(Debug, serde::Deserialize)]
pub struct Config<St: Storage, No: Notifier, Si: Sink, Cmd: CommandSink> {
    pub storage: St::Config,
    pub notifier: No::Config,
    pub sink: Si::Config,
    pub command_sink: Cmd::Config,
}

#[derive(Clone, Debug, Default)]
pub struct UpdateOptions {
    pub ignore_unclean_inbox: bool,
}

impl<St: Storage, No: Notifier, Si: Sink, Cmd: CommandSink> Clone for Config<St, No, Si, Cmd> {
    fn clone(&self) -> Self {
        Self {
            storage: self.storage.clone(),
            notifier: self.notifier.clone(),
            sink: self.sink.clone(),
            command_sink: self.command_sink.clone(),
        }
    }
}

pub const POSTPONE_DURATION: std::time::Duration = std::time::Duration::from_secs(5);

pub struct Service<St: Storage, No: Notifier, Si: Sink, Cmd: CommandSink> {
    storage: St,
    notifier: No,
    sink: Si,
    command_sink: Cmd,
    postpone: Duration,
}

#[derive(Debug)]
pub enum OutboxState {
    // The outbox is clean
    Clean,
    // The outbox is not clean, but cannot be retried now
    Unclean,
    // The outbox is not clean, but can be retried now
    Retry,
}

impl<St: Storage, No: Notifier, Si: Sink, Cmd: CommandSink> Service<St, No, Si, Cmd> {
    pub fn from_config(
        spawner: &mut dyn Spawner,
        config: Config<St, No, Si, Cmd>,
    ) -> anyhow::Result<Self> {
        let Config {
            storage,
            notifier,
            sink,
            command_sink,
        } = config;
        let storage = St::from_config(&storage)?;
        let notifier = No::from_config(&notifier)?;
        let sink = Si::from_config(sink)?;
        let command_sink = Cmd::from_config(spawner, command_sink)?;
        Ok(Self::new(storage, notifier, sink, command_sink))
    }

    pub fn new(storage: St, notifier: No, sink: Si, command_sink: Cmd) -> Self {
        Self {
            storage,
            notifier,
            sink,
            command_sink,
            postpone: Duration::seconds(POSTPONE_DURATION.as_secs() as i64),
        }
    }

    pub fn sink(&self) -> &Si {
        &self.sink
    }

    pub async fn create(&self, thing: Thing) -> Result<Thing, Error<St, No, Cmd>> {
        let Outcome {
            mut new_thing,
            outbox,
            commands,
        } = Machine::create(thing).await?;

        OUTBOX_EVENTS.inc_by(outbox.len() as u64);
        Self::add_outbox(&mut new_thing, outbox);

        let new_thing = self
            .storage
            .create(new_thing)
            .await
            .map_err(Error::Storage)?;

        // we can send the events right away, as we created the entry

        let new_thing = self.send_and_ack(new_thing).await?;

        // send commands

        self.command_sink
            .send_commands(commands)
            .await
            .map_err(Error::Command)?;

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

    pub async fn get(&self, id: &Id) -> Result<Option<Thing>, Error<St, No, Cmd>> {
        self.storage
            .get(&id.application, &id.thing)
            .await
            .or_else(|err| match err {
                storage::Error::NotFound => Ok(None),
                _ => Err(err),
            })
            .map_err(Error::Storage)
    }

    pub async fn delete(
        &self,
        id: &Id,
        opts: Option<&Preconditions<'_>>,
    ) -> Result<bool, Error<St, No, Cmd>> {
        // get the current thing

        log::debug!("Deleting thing: {id}");

        let mut thing = match self.storage.get(&id.application, &id.thing).await {
            Ok(Some(thing)) => thing,
            // not found, we are done here
            Err(storage::Error::NotFound) | Ok(None) => return Ok(false),
            Err(err) => return Err(Error::Storage(err)),
        };

        if let Some(opts) = opts {
            if !opts.matches(&thing) {
                return Err(Error::Storage(storage::Error::PreconditionFailed));
            }
        }

        if thing.metadata.deletion_timestamp.is_some() {
            // already marked as deleted
            return Ok(false);
        }

        let original_empty = thing
            .internal
            .as_ref()
            .map(|internal| internal.outbox.is_empty())
            .unwrap_or(true);

        // mark deleted
        thing.metadata.deletion_timestamp = Some(Utc::now());

        // run machine for deletion
        let DeletionOutcome { mut thing, outbox } = Machine::delete(thing).await?;
        // add outbox
        Self::add_outbox(&mut thing, outbox);
        // check if the thing's outbox contains events
        if !thing.outbox().is_empty() {
            // if so, store, which also stores the deletion marker
            thing = self.storage.update(thing).await.map_err(Error::Storage)?;
            // from here on, we are marked deleted and need to re-process if we fail with the next step
            if original_empty {
                // it was originally empty, so we can send and ack, otherwise the waker will
                // trigger (and delete) us later
                thing = self.send_and_ack(thing).await?;
            }
        }

        if thing.outbox().is_empty() {
            // if the outbox is empty, delete
            self.storage
                .delete_with(
                    &id.application,
                    &id.thing,
                    Preconditions {
                        resource_version: thing.metadata.resource_version.as_deref(),
                        uid: thing.metadata.uid.as_deref(),
                    },
                )
                .await
                .or_else(|err| match err {
                    // if we didn't find what we want to delete, this is just fine
                    storage::Error::NotFound => Ok(false),
                    err => Err(Error::Storage(err)),
                })?;
        }

        // notify
        self.notifier
            .notify(&thing)
            .await
            .map_err(Error::Notifier)?;

        // done
        Ok(true)
    }

    pub async fn update<U>(
        &self,
        id: &Id,
        updater: &U,
        opts: &UpdateOptions,
    ) -> Result<Thing, Error<St, No, Cmd>>
    where
        U: Updater,
    {
        log::debug!("Updating thing: {id}");

        let current_thing = self
            .storage
            .get(&id.application, &id.thing)
            .await
            .and_then(|r| r.ok_or(storage::Error::NotFound))
            .map_err(Error::Storage)?;

        if current_thing.metadata.deletion_timestamp.is_some() {
            // if we are already deleted, we don't do any more updates. Except processing
            // outgoing events.

            // check for unprocessed events
            self.check_unprocessed_events(current_thing, false).await?;

            // returned with "ok", so all events have been processed, we can now delete

            // now return with "not found"
            return Err(Error::Storage(storage::Error::NotFound));
        }

        // check for unprocessed events
        let current_thing = self
            .check_unprocessed_events(current_thing, opts.ignore_unclean_inbox)
            .await?;

        let Outcome {
            mut new_thing,
            outbox,
            commands,
        } = Machine::new(current_thing.clone())
            .update(|thing| async { updater.update(thing) })
            .await?;

        OUTBOX_EVENTS.inc_by(outbox.len() as u64);
        COMMANDS.inc_by(commands.len() as u64);
        Self::add_outbox(&mut new_thing, outbox);

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

        // waker is scheduled by add_outbox before storing
        let current_outbox = current_thing
            .internal
            .as_ref()
            .map(|i| i.outbox.len())
            .unwrap_or(0);

        log::debug!("Current outbox size: {}", current_outbox);

        if current_outbox == 0 {
            // only send when we had no previous events, otherwise we already queued
            new_thing = self.send_and_ack(new_thing).await?;
        }

        // send commands

        self.command_sink
            .send_commands(commands)
            .await
            .map_err(Error::Command)?;

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
    fn add_outbox(thing: &mut Thing, outbox: Vec<OutboxMessage>) {
        // get internal section

        let internal = {
            // TODO: replace with thing.internal.get_or_insert_default(); once it is stabilized
            if thing.internal.is_none() {
                thing.internal = Some(Default::default());
            }
            // unwrap is safe here, as we just set it to "some"
            thing.internal.as_mut().unwrap()
        };

        // schedule waker, if required

        if !internal.outbox.is_empty() || !outbox.is_empty() {
            // we have something, set waker
            internal.wakeup(Duration::seconds(30), WakerReason::Outbox);
        }

        if outbox.is_empty() {
            // early return, as we don't need to modify anything
            return;
        }

        let add: Vec<_> = outbox
            .into_iter()
            .map(|message| Event {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now(),
                application: thing.metadata.application.clone(),
                thing: message.thing,
                message: message.message,
            })
            .collect();

        // append events to the stored outbox

        internal.outbox.extend(add);
    }

    async fn send_and_ack(&self, mut new_thing: Thing) -> Result<Thing, Error<St, No, Cmd>> {
        let outbox = if let Some(outbox) = new_thing.internal.as_ref().map(|i| &i.outbox) {
            outbox
        } else {
            // no internal section -> no events
            return Ok(new_thing);
        };

        log::debug!("New outbox: {outbox:?}");

        if outbox.is_empty() {
            // early return
            return Ok(new_thing);
        }

        let outbox = outbox.to_vec();

        match self.sink.publish_iter(outbox).await {
            Ok(()) => {
                log::debug!("All outbox events sent");

                // ack outbox events
                if let Some(internal) = &mut new_thing.internal {
                    internal.outbox.clear();

                    // we can clear the waker, as we are sure that the outbox was clear initially
                    internal.clear_wakeup(WakerReason::Outbox);
                    // and store
                    new_thing = self
                        .storage
                        .update(new_thing)
                        .await
                        .map_err(Error::Storage)?;
                }
            }
            Err((0, err)) => {
                log::info!("Failed to send any outbox event: {err:?}");
                // Special case, none had been successful. Might actually be to most common case.
                // And we don't need to do anything.
            }
            Err((done, err)) => {
                log::info!("Failed to send some outbox events: {err:?}, done: {done}");

                // ack done events
                if let Some(internal) = &mut new_thing.internal {
                    // remove the first, done elements
                    internal.outbox = internal.outbox.split_off(done);

                    // waker is already set, so just store
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

    /// If there are unprocessed events, process them now.
    ///
    /// Return a new "current thing" refreshed from the storage.
    async fn check_unprocessed_events(
        &self,
        mut current_thing: Thing,
        ignore_unclean_inbox: bool,
    ) -> Result<Thing, Error<St, No, Cmd>> {
        // we keep looping, until either we:
        // 1) Have a clean inbox
        // 2) Find out we can't process any more events
        loop {
            let (thing, state) = self.prepare_outbox(current_thing).await?;

            log::debug!("Outbox state: {state:?}");

            match state {
                OutboxState::Clean => break Ok(thing),
                OutboxState::Unclean if ignore_unclean_inbox => break Ok(thing),
                OutboxState::Unclean => break Err(Error::UncleanOutbox),
                OutboxState::Retry => {
                    current_thing = self.send_and_ack(thing).await?;
                    log::debug!("Thing after trying: {current_thing:?}");
                }
            }
        }
    }

    /// Get the unprocessed events, which are eligible to be sent now. And advance their timestamp.
    ///
    /// If that was possible, then no one else will pick them up as ready to send, and we can
    /// proceed. This also means, that no one else should modify the resource, as there are
    /// unprocessed events pending, which cannot be processed, from their point of view.
    ///
    /// Next we try to send, and clear the events.
    async fn prepare_outbox(
        &self,
        mut thing: Thing,
    ) -> Result<(Thing, OutboxState), Error<St, No, Cmd>> {
        let internal = match &mut thing.internal {
            None => return Ok((thing, OutboxState::Clean)),
            Some(internal) if internal.outbox.is_empty() => return Ok((thing, OutboxState::Clean)),
            Some(internal) => internal,
        };

        // same cutoff timestamp for all
        let now = Utc::now();

        // check if we can retry the outbox
        for event in &mut internal.outbox {
            if event.timestamp > now {
                return Ok((thing, OutboxState::Unclean));
            }
        }

        // we can, now we can modify the state
        for event in &mut internal.outbox {
            // advance timestamp into the future
            event.timestamp = event.timestamp + self.postpone;
        }

        // store thing with updated event timestamps, only then we may proceed.
        let thing = self
            .storage
            .update(thing.clone())
            .await
            .map_err(Error::Storage)?;

        // return result, ready to send events
        Ok((thing, OutboxState::Retry))
    }
}
