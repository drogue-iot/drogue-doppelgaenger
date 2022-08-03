pub mod kafka;

use crate::model::Thing;
use crate::service::Id;
use async_trait::async_trait;
use std::fmt::Debug;

#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
    #[error("Sender: {0}")]
    Sender(#[source] E),
}

#[async_trait]
pub trait Notifier: Sized + Send + Sync + 'static {
    type Config: Clone + Debug + Send + Sync + serde::de::DeserializeOwned + 'static;
    type Error: std::error::Error + Debug + Send + Sync;

    fn from_config(config: &Self::Config) -> anyhow::Result<Self>;

    async fn notify(&self, thing: &Thing) -> Result<(), Error<Self::Error>>;
}
