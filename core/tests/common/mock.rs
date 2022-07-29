#![allow(unused)]

use crate::common::failure::{Failure, FailureProvider};
use anyhow::{anyhow, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use drogue_doppelgaenger_core::{
    model::{Thing, WakerReason},
    notifier::Notifier,
    processor::{sink::Sink, source::Source, Event, Processor},
    service::{Id, Service},
    storage::{Error, Storage},
    waker::{self, TargetId, Waker},
};
use std::collections::{btree_map::Entry, BTreeMap, HashMap};
use std::convert::Infallible;
use std::future::Future;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::{oneshot, Mutex, RwLock};
use tokio::task::JoinHandle;
use uuid::Uuid;

#[derive(Clone)]
pub struct MockStorage {
    pub application: String,
    pub things: Arc<RwLock<BTreeMap<String, Thing>>>,
    waker: MockWaker,
}

impl MockStorage {
    pub fn new<A: Into<String>>(application: A, waker: MockWaker) -> Self {
        Self {
            application: application.into(),
            things: Default::default(),
            waker,
        }
    }
}

#[async_trait]
impl Storage for MockStorage {
    type Config = ();
    type Error = Infallible;

    fn from_config(_: &Self::Config) -> anyhow::Result<Self> {
        panic!("Not supported")
    }

    async fn get(
        &self,
        application: &str,
        name: &str,
    ) -> Result<Option<Thing>, Error<Self::Error>> {
        if application != self.application {
            return Ok(None);
        }

        return Ok(self.things.read().await.get(name).cloned());
    }

    async fn create(&self, mut thing: Thing) -> Result<Thing, Error<Self::Error>> {
        if thing.metadata.application != self.application {
            return Err(Error::NotAllowed);
        }

        // fake metadata

        thing.metadata.creation_timestamp = Some(Utc::now());
        thing.metadata.uid = Some(Uuid::new_v4().to_string());
        thing.metadata.resource_version = Some(Uuid::new_v4().to_string());
        thing.metadata.generation = Some(1);

        // store

        let mut lock = self.things.write().await;

        if lock.contains_key(&thing.metadata.name) {
            return Err(Error::AlreadyExists);
        }

        lock.insert(thing.metadata.name.clone(), thing.clone());

        // while still holding the lock

        self.waker.update(&thing).await;

        // done

        Ok(thing)
    }

    async fn update(&self, mut thing: Thing) -> Result<Thing, Error<Self::Error>> {
        if thing.metadata.application != self.application {
            return Err(Error::NotAllowed);
        }

        let mut things = self.things.write().await;

        let result = match things.entry(thing.metadata.name.clone()) {
            Entry::Occupied(mut entry) => {
                // check pre-conditions
                match (&thing.metadata.uid, &entry.get().metadata.uid) {
                    (None, _) => {}
                    (Some(expected), Some(actual)) if expected == actual => {}
                    _ => return Err(Error::PreconditionFailed),
                }
                match (
                    &thing.metadata.resource_version,
                    &entry.get().metadata.resource_version,
                ) {
                    (None, _) => {}
                    (Some(expected), Some(actual)) if expected == actual => {}
                    _ => return Err(Error::PreconditionFailed),
                }

                // immutable fields
                thing.metadata.creation_timestamp = entry.get().metadata.creation_timestamp;
                thing.metadata.uid = entry.get().metadata.uid.clone();

                // updated by storage
                thing.metadata.resource_version = Some(Uuid::new_v4().to_string());
                thing.metadata.generation =
                    Some(entry.get().metadata.generation.unwrap_or_default() + 1);

                // store
                entry.insert(thing.clone());

                // while still holding the lock
                self.waker.update(&thing).await;

                // return
                Ok(thing)
            }
            Entry::Vacant(_) => Err(Error::NotFound),
        };

        result
    }

    async fn delete(&self, application: &str, name: &str) -> Result<bool, Error<Self::Error>> {
        if application != self.application {
            return Ok(false);
        }

        Ok(self.things.write().await.remove(name).is_some())
    }
}

#[derive(Clone)]
pub struct MockNotifier {
    pub events: Arc<RwLock<Vec<Thing>>>,
}

impl MockNotifier {
    pub fn new() -> Self {
        Self {
            events: Default::default(),
        }
    }

    pub async fn drain(&mut self) -> Vec<Thing> {
        let mut lock = self.events.write().await;
        lock.drain(..).collect()
    }
}

#[async_trait]
impl Notifier for MockNotifier {
    type Config = ();
    type Error = Infallible;

    fn from_config(_: &Self::Config) -> anyhow::Result<Self> {
        Ok(Self::new())
    }

    async fn notify(
        &self,
        thing: &Thing,
    ) -> Result<(), drogue_doppelgaenger_core::notifier::Error<Self::Error>> {
        self.events.write().await.push(thing.clone());
        Ok(())
    }
}

#[derive(Debug)]
struct Item {
    event: Event,
    notifier: oneshot::Sender<()>,
}

#[derive(Clone)]
pub struct MockSource {
    tx: Sender<Item>,
    rx: Arc<Mutex<Option<Receiver<Item>>>>,
}

impl MockSource {
    pub fn new() -> Self {
        let (tx, rx) = channel(100);
        Self {
            tx,
            rx: Arc::new(Mutex::new(Some(rx))),
        }
    }
}

#[async_trait]
impl Source for MockSource {
    type Config = ();

    fn from_config(_: Self::Config) -> anyhow::Result<Self> {
        panic!("Not supported")
    }

    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: Fn(Event) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send,
    {
        // drop our own sender, so that we don't keep the loop running
        drop(self.tx);

        let mut rx = self
            .rx
            .lock()
            .await
            .take()
            .expect("Mock source can only be started once");

        // just forward events
        while let Some(item) = rx.recv().await {
            f(item.event).await?;
            let _ = item.notifier.send(());
        }

        // when all other senders close, we return

        Ok(())
    }
}

#[derive(Clone)]
pub struct MockSink {
    failure: Failure<(), anyhow::Error>,
    pub events: Arc<RwLock<Vec<Event>>>,
    tx: Sender<Event>,
    rx: Arc<Mutex<Option<Receiver<Event>>>>,
}

impl MockSink {
    pub fn new(failure: Failure<(), anyhow::Error>) -> Self {
        let (tx, rx) = channel(100);
        Self {
            failure,
            events: Arc::default(),
            tx,
            rx: Arc::new(Mutex::new(Some(rx))),
        }
    }

    async fn take_receiver(&self) -> Option<Receiver<Event>> {
        self.rx.lock().await.take()
    }

    pub async fn drain(&mut self) -> Vec<Event> {
        self.events.write().await.drain(..).collect()
    }
}

#[async_trait]
impl Sink for MockSink {
    type Config = ();

    fn from_config(_: Self::Config) -> anyhow::Result<Self> {
        panic!("Not supported")
    }

    async fn publish(&self, event: Event) -> anyhow::Result<()> {
        log::debug!("MockSink: {event:?}");

        self.failure.failed(())?;

        self.tx.send(event.clone()).await?;
        self.events.write().await.push(event);
        Ok(())
    }
}

pub struct Context {
    pub sink: MockSink,
    pub source: MockSourceFeeder,
    pub notifier: MockNotifier,
    pub service: Service<MockStorage, MockNotifier, MockSink>,
    pub processor: Processor<MockStorage, MockNotifier, MockSink, MockSource>,
    pub waker: waker::Processor<MockWaker, MockSink>,
}

impl Context {
    /// Run the context.
    ///
    /// When the `feed_events` parameter is true, events sent to the sink will automatically be fed
    /// into the source again.
    pub fn run(self, feed_events: bool) -> RunningContext {
        let processor = self.processor;
        let waker = self.waker;

        let feed_runner = if feed_events {
            // forward events from sink to source
            let sink = self.sink.clone();
            let source = self.source.clone();
            Some(Handle::current().spawn(async move {
                let mut rx = sink.take_receiver().await.expect("Must have receiver");
                while let Some(event) = rx.recv().await {
                    source.send(event).await.unwrap();
                }
            }))
        } else {
            None
        };

        RunningContext {
            sink: self.sink,
            notifier: self.notifier,
            service: self.service,
            runner: ContextRunner {
                source: self.source,
                processor_runner: Handle::current().spawn(async move { processor.run().await }),
                waker_runner: Handle::current().spawn(async move { waker.run().await }),
                feed_runner,
            },
        }
    }
}

pub struct ContextRunner {
    source: MockSourceFeeder,
    processor_runner: JoinHandle<anyhow::Result<()>>,
    waker_runner: JoinHandle<anyhow::Result<()>>,
    feed_runner: Option<JoinHandle<()>>,
}

impl ContextRunner {
    /// safely shut down a running context
    pub async fn shutdown(mut self) -> anyhow::Result<()> {
        // abort the feed runner if we have one
        if let Some(feed_runner) = self.feed_runner.take() {
            feed_runner.abort();
        }

        // we abort the waker
        self.waker_runner.abort();

        // drop the source to shut down the loop
        drop(self.source);
        // await the end of the loop
        self.processor_runner.await??;

        // done
        Ok(())
    }
}

impl Deref for ContextRunner {
    type Target = MockSourceFeeder;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

pub struct RunningContext {
    pub sink: MockSink,
    pub notifier: MockNotifier,
    pub service: Service<MockStorage, MockNotifier, MockSink>,
    pub runner: ContextRunner,
}

#[derive(Clone)]
pub struct MockSourceFeeder {
    source: MockSource,
}

impl MockSourceFeeder {
    /// Send an event, and wait until it is processed by the processor.
    ///
    /// NOTE: This will block indefinitely, if the processor is not running!
    pub async fn send_wait(&self, event: Event) -> anyhow::Result<()> {
        self.send(event).await?.await?;
        Ok(())
    }

    pub async fn send(&self, event: Event) -> anyhow::Result<oneshot::Receiver<()>> {
        let (tx, rx) = oneshot::channel();
        let item = Item {
            event,
            notifier: tx,
        };

        self.source.tx.send(item).await?;

        Ok(rx)
    }
}

#[derive(Clone, Default)]
pub struct MockWaker {
    wakers: Arc<Mutex<HashMap<String, (DateTime<Utc>, Vec<WakerReason>, TargetId)>>>,
}

impl MockWaker {
    pub fn new() -> Self {
        Self {
            wakers: Default::default(),
        }
    }

    pub async fn update(&self, thing: &Thing) {
        let mut lock = self.wakers.lock().await;

        let waker = thing.waker();
        match waker.when {
            Some(when) => {
                let id = TargetId {
                    id: Id::new(
                        thing.metadata.application.clone(),
                        thing.metadata.name.clone(),
                    ),
                    resource_version: thing
                        .metadata
                        .resource_version
                        .as_ref()
                        .cloned()
                        .unwrap_or_default(),
                    uid: thing.metadata.uid.as_ref().cloned().unwrap_or_default(),
                };
                lock.insert(
                    thing.metadata.name.clone(),
                    (when, waker.why.iter().map(|r| *r).collect::<Vec<_>>(), id),
                );
            }
            None => {
                lock.remove(&thing.metadata.name);
            }
        }
    }
}

#[async_trait]
impl Waker for MockWaker {
    type Config = ();

    fn from_config(_: Self::Config) -> anyhow::Result<Self> {
        panic!("Not supported");
    }

    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: Fn(TargetId, Vec<WakerReason>) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send,
    {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            interval.tick().await;

            log::debug!("MockWaker - ticking");

            // this is a bit messy, might be improved with drain_filter

            let expired = {
                let mut lock = self.wakers.lock().await;

                let mut expired_keys = Vec::new();
                for (k, v) in lock.iter() {
                    if v.0 < Utc::now() {
                        expired_keys.push(k.clone());
                    }
                }

                let mut expired = Vec::new();

                for k in expired_keys {
                    if let Some(v) = lock.remove(&k) {
                        expired.push(v);
                    }
                }

                expired
            };

            // call back outside the lock

            for v in expired {
                log::info!("Mock waker for: {v:?}");
                let _ = f(v.2, v.1).await;
            }
        }
    }
}

pub struct Builder {
    sink_failure: Failure<(), anyhow::Error>,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            sink_failure: Default::default(),
        }
    }

    pub fn sink_failure(mut self, f: Failure<(), anyhow::Error>) -> Self {
        self.sink_failure = f;
        self
    }

    pub fn setup(self) -> Context {
        let _ = env_logger::builder().is_test(true).try_init();

        let sink = MockSink::new(self.sink_failure.clone());
        let source = MockSource::new();
        let notifier = MockNotifier::new();

        let waker = MockWaker::new();
        let storage = MockStorage::new("default", waker.clone());

        let processor = Processor::new(
            Service::new(storage.clone(), notifier.clone(), sink.clone()),
            source.clone(),
        );

        let waker = waker::Processor::new(waker, sink.clone());

        let service = Service::new(storage.clone(), notifier.clone(), sink.clone());

        let source = MockSourceFeeder { source };

        Context {
            sink,
            service,
            processor,
            notifier,
            source,
            waker,
        }
    }
}

pub fn setup() -> Context {
    Builder::new().setup()
}
