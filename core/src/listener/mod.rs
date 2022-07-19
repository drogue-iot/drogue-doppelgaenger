//! This needs restructuring

use crate::config::kafka::KafkaProperties;
use crate::{model::Thing, notifier::kafka, service::Id};
use anyhow::Context;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::Message as _;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use tokio::runtime::Handle;
use tokio::sync::broadcast::{channel, Sender};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;

/// A notifier source using Kafka.
pub struct KafkaSource {
    task: JoinHandle<()>,
    inner: Arc<RwLock<Inner>>,
}

struct Inner {
    listeners: BTreeMap<String, (usize, Sender<Message>)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Change(Arc<Thing>),
}

pub struct Source {
    id: String,
    rx: Pin<Box<dyn futures::Stream<Item = Result<Message, BroadcastStreamRecvError>>>>,
    inner: Arc<RwLock<Inner>>,
}

impl Deref for Source {
    type Target = Pin<Box<dyn futures::Stream<Item = Result<Message, BroadcastStreamRecvError>>>>;

    fn deref(&self) -> &Self::Target {
        &self.rx
    }
}

impl DerefMut for Source {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.rx
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        let mut lock = self.inner.write().unwrap();

        let remove = if let Some(entry) = lock.listeners.get_mut(&self.id) {
            entry.0 -= 1;
            entry.0 == 0
        } else {
            false
        };
        if remove {
            lock.listeners.remove(&self.id);
        }
    }
}

fn find_id<'m>(msg: &'m BorrowedMessage) -> Option<&'m str> {
    msg.key_view().transpose().ok().flatten()
}

impl KafkaSource {
    pub fn new(config: kafka::Config) -> anyhow::Result<Self> {
        log::info!("Starting Kafka event source: {config:?}");

        let topic = config.topic;

        let config: rdkafka::ClientConfig = KafkaProperties(config.properties).into();
        let consumer: StreamConsumer = config.create().context("Creating consumer")?;

        consumer.subscribe(&[&topic]).context("Start subscribe")?;

        let inner = Inner {
            listeners: Default::default(),
        };
        let inner = Arc::new(RwLock::new(inner));

        let consumer = Self::run(consumer, inner.clone());

        let task = Handle::current().spawn(consumer);

        Ok(Self { task, inner })
    }

    async fn run(consumer: StreamConsumer, inner: Arc<RwLock<Inner>>) {
        log::info!("Running Kafka listener ...");

        loop {
            let msg = consumer.recv().await;
            match msg {
                Err(err) => {
                    log::error!("Failed to read from Kafka: {err}");
                    break;
                }
                Ok(msg) => {
                    let id = find_id(&msg);
                    log::info!("Thing id: {id:?}");
                    if let Some(id) = id {
                        let lock = inner.read().unwrap();
                        if let Some(listener) = lock.listeners.get(id) {
                            if let Some(Ok(thing)) = msg
                                .payload()
                                .map(|payload| serde_json::from_slice::<Thing>(payload))
                            {
                                if let Err(err) = listener.1.send(Message::Change(Arc::new(thing)))
                                {
                                    log::info!("Failed to broadcast change: {err:?}");
                                }
                            }
                        }
                    }
                }
            }
        }

        // FIXME: as tokio swallows up panics, we need to bring this to the main method
        panic!("Kafka listener exited!");
    }

    pub fn subscribe(&self, id: Id) -> Source {
        let mut lock = self.inner.write().unwrap();
        let rx = match lock.listeners.entry(id.to_string()) {
            Entry::Vacant(entry) => {
                let (tx, rx) = channel(10);
                entry.insert((1, tx));
                rx
            }
            Entry::Occupied(mut entry) => {
                let value = entry.get_mut();
                value.0 += 1;
                value.1.subscribe()
            }
        };

        Source {
            id: id.to_string(),
            rx: Box::pin(BroadcastStream::new(rx)),
            inner: self.inner.clone(),
        }
    }
}

impl Drop for KafkaSource {
    fn drop(&mut self) {
        self.task.abort();
    }
}
