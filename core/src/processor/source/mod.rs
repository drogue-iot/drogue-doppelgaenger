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
pub trait Source: Sized + Send + Sync {
    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: FnMut(Event) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send;
}

#[async_trait]
pub trait Sink: Sized + Send + Sync + Clone + 'static {
    async fn publish(&self, event: Event) -> anyhow::Result<()>;

    async fn publish_iter<I>(&self, i: I) -> Result<(), (usize, anyhow::Error)>
    where
        I: IntoIterator<Item = Event> + Send + Sync,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        let mut n = 0;
        for event in i.into_iter() {
            if let Err(err) = self.publish(event).await {
                return Err((n, err.into()));
            }
            n += 1;
        }

        Ok(())
    }
}
