pub mod kafka;

use crate::processor::Event;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::fmt::Debug;
use std::future::Future;

#[async_trait]
pub trait Source: Sized + Send + Sync {
    type Config: Clone + Debug + DeserializeOwned;

    fn from_config(config: Self::Config) -> anyhow::Result<Self>;

    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: Fn(Event) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send;
}
