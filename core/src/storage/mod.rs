pub mod postgres;

use crate::model::{Metadata, Thing};
use async_trait::async_trait;
use std::fmt::Debug;
use std::future::Future;

#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
    /// Returned when an option should modify a thing, but it could not be found.
    ///
    /// Not used, when not finding the things isn't a problem.
    #[error("Not found")]
    NotFound,
    #[error("Not allowed")]
    NotAllowed,
    #[error("Already exists")]
    AlreadyExists,
    #[error("Precondition failed")]
    PreconditionFailed,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Internal Error: {0}")]
    Internal(#[source] E),
    #[error("{0}")]
    Generic(String),
}

#[derive(thiserror::Error)]
pub enum UpdateError<SE, UE> {
    #[error("Service error: {0}")]
    Service(#[from] Error<SE>),
    #[error("Mutator error: {0}")]
    Mutator(#[source] UE),
}

#[async_trait]
pub trait Storage: Sized + Send + Sync + 'static {
    type Config: Clone + Debug + Send + Sync + serde::de::DeserializeOwned + 'static;
    type Error: std::error::Error + Debug;

    fn from_config(config: &Self::Config) -> anyhow::Result<Self>;

    async fn get(&self, application: &str, name: &str)
        -> Result<Option<Thing>, Error<Self::Error>>;
    async fn create(&self, thing: Thing) -> Result<Thing, Error<Self::Error>>;
    async fn update(&self, thing: Thing) -> Result<Thing, Error<Self::Error>>;

    async fn patch<F, Fut, E>(
        &self,
        application: &str,
        name: &str,
        f: F,
    ) -> Result<Thing, UpdateError<Self::Error, E>>
    where
        F: FnOnce(Thing) -> Fut + Send + Sync,
        Fut: Future<Output = Result<Thing, E>> + Send + Sync,
        E: Send + Sync,
    {
        log::debug!("Updating existing thing: {application} / {name}");

        let current_thing = self.get(application, name).await?.ok_or(Error::NotFound)?;
        // capture current metadata
        let Metadata {
            name,
            application,
            uid,
            creation_timestamp,
            resource_version,
            generation,
            annotations: _,
            labels: _,
        } = current_thing.metadata.clone();
        let mut new_thing = f(current_thing.clone())
            .await
            .map_err(UpdateError::Mutator)?;

        // override metadata which must not be changed by the caller
        new_thing.metadata = Metadata {
            name,
            application,
            uid,
            creation_timestamp,
            resource_version,
            generation,
            ..new_thing.metadata
        };

        if current_thing == new_thing {
            // no change
            return Ok(current_thing);
        }

        // perform update

        self.update(new_thing).await.map_err(UpdateError::Service)
    }

    /// Delete a thing. Return `true` if the thing was deleted, `false` if it didn't exist.
    async fn delete(&self, application: &str, name: &str) -> Result<bool, Error<Self::Error>>;
}
