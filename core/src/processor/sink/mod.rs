pub mod kafka;

use crate::processor::Event;
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use std::fmt::Debug;

#[async_trait]
pub trait Sink: Sized + Send + Sync + Clone + 'static {
    type Config: Clone + Debug + DeserializeOwned;

    fn from_config(config: Self::Config) -> anyhow::Result<Self>;

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
