pub mod postgres;

use crate::{
    model::WakerReason,
    processor::{sink::Sink, Event, Message},
    service::Id,
};
use async_trait::async_trait;
use std::future::Future;

#[derive(Clone, Debug)]
pub struct TargetId {
    pub id: Id,
    pub uid: String,
    pub resource_version: String,
}

#[async_trait]
pub trait Waker: Sized + Send + Sync {
    type Config;

    fn from_config(config: Self::Config) -> anyhow::Result<Self>;

    /// Run the waker
    ///
    /// The function provided must wake up the thing. It must only return ok if it was able to do so.
    /// It is not necessary to direct reconcile the thing though.
    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: Fn(TargetId, Vec<WakerReason>) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send;
}

pub struct Config<W: Waker, S: Sink> {
    pub waker: W::Config,
    pub sink: S::Config,
}

/// Process wakeups
pub struct Processor<W: Waker, S: Sink> {
    waker: W,
    sink: S,
}

impl<W, S> Processor<W, S>
where
    W: Waker,
    S: Sink,
{
    pub fn new(waker: W, sink: S) -> Self {
        Self { waker, sink }
    }

    pub fn from_config(config: Config<W, S>) -> anyhow::Result<Self> {
        Ok(Self::new(
            W::from_config(config.waker)?,
            S::from_config(config.sink)?,
        ))
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let sink = self.sink;
        let waker = self.waker;

        waker
            .run(|id, reasons| async {
                sink.publish(Event::new(
                    id.id.application,
                    id.id.thing,
                    Message::Wakeup { reasons },
                ))
                .await?;

                Ok(())
            })
            .await?;

        Ok(())
    }
}
