pub mod postgres;

use crate::model::Thing;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::fmt::Debug;
use std::future::Future;

#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
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

    fn new(config: &Self::Config) -> anyhow::Result<Self>;

    async fn get(&self, application: &str, name: &str) -> Result<Thing, Error<Self::Error>>;
    async fn create(&self, thing: &Thing) -> Result<(), Error<Self::Error>>;
    async fn update(&self, thing: &Thing) -> Result<(), Error<Self::Error>>;
    async fn patch<F, Fut, E>(
        &self,
        application: &str,
        name: &str,
        f: F,
    ) -> Result<(), UpdateError<Self::Error, E>>
    where
        F: FnOnce(Thing) -> Fut + Send + Sync,
        Fut: Future<Output = Result<Thing, E>> + Send + Sync,
        E: Send + Sync;
    async fn delete(&self, application: &str, name: &str) -> Result<(), Error<Self::Error>>;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Internal {
    pub wakeup: Option<DateTime<Utc>>,
}
