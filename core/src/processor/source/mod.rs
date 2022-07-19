pub mod kafka;

use crate::processor::Event;
use async_trait::async_trait;
use std::future::Future;

#[async_trait]
pub trait EventStream {
    type Config;
    type Source: Source;
    type Sink: Sink;

    fn new(config: Self::Config) -> anyhow::Result<(Self::Source, Self::Sink)>;
}

#[async_trait]
pub trait Source: Sized {
    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: FnMut(Event) -> Fut + Send + Sync,
        Fut: Future<Output = ()> + Send;
}

#[async_trait]
pub trait Sink: Sized + Clone {
    async fn publish(&mut self, event: Event) -> anyhow::Result<()>;
}
