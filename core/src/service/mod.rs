mod error;
mod id;
mod updater;

pub use error::*;
pub use id::Id;
use std::convert::Infallible;
pub use updater::*;

use crate::machine::Machine;
use crate::model::Thing;
use crate::notifier::Notifier;
use crate::storage::{self, Storage};

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

pub struct Service<S: Storage, N: Notifier> {
    storage: S,
    notifier: N,
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

impl<S: Storage, N: Notifier> Service<S, N> {
    pub fn new(config: Config<S, N>) -> anyhow::Result<Self> {
        let Config { storage, notifier } = config;
        let storage = S::new(&storage)?;
        let notifier = N::new(&notifier)?;
        Ok(Self { storage, notifier })
    }

    pub async fn create(&self, thing: Thing) -> Result<(), Error<S, N>> {
        let thing = Machine::create(thing).await?;

        Ok(self.storage.create(&thing).await.map_err(Error::Storage)?)
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

    pub async fn update<U>(&self, id: Id, updater: U) -> Result<(), Error<S, N>>
    where
        U: Updater,
    {
        let current_thing = self
            .storage
            .get(&id.application, &id.thing)
            .await
            .map_err(Error::Storage)?;

        let mut new_thing = Machine::new(current_thing.clone())
            .update(|thing| async { updater.update(thing) })
            .await?;

        if current_thing == new_thing {
            // no change, nothing to do
            return Ok(());
        }

        if let Some(generation) = &mut new_thing.metadata.generation {
            // now we can increase the generation
            *generation += 1;
        }

        // store

        self.storage
            .update(&new_thing)
            .await
            .map_err(Error::Storage)?;

        // TODO: send patch events
        // TODO: ack patch events

        // notify

        self.notifier
            .notify(&new_thing)
            .await
            .map_err(Error::Notifier)?;

        // FIXME: handle failure

        // done

        Ok(())
    }
}
