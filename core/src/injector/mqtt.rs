use crate::{
    injector::{
        metadata::{Meta, MetadataMapper},
        payload::PayloadMapper,
    },
    mqtt::MqttClient,
    processor::{sink::Sink, Event},
};
use anyhow::bail;
use async_trait::async_trait;
use chrono::Utc;
use lazy_static::lazy_static;
use prometheus::{register_histogram, register_int_counter_vec, Histogram, IntCounterVec};
use rumqttc::{AsyncClient, EventLoop, Incoming, Publish, QoS, SubscribeReasonCode};
use tracing::instrument;

lazy_static! {
    static ref EVENTS: IntCounterVec = register_int_counter_vec!(
        "injector_events",
        "Number of events processed by injector",
        &["result"]
    )
    .unwrap();
    static ref LAG: Histogram =
        register_histogram!("injector_lag", "Lag in ms for the injector").unwrap();
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub client: MqttClient,

    pub topic: String,
}

impl Config {
    pub async fn run<T: Target>(self, target: T) -> anyhow::Result<()> {
        Injector::new(self, target)?.run().await
    }
}

#[async_trait]
pub trait Target {
    /// An event was received from the injector and needs to be processed.
    async fn event(&self, event: cloudevents::Event) -> anyhow::Result<()>;
}

/// An injector target based on a [`Sink`].
pub struct SinkTarget<S: Sink> {
    pub sink: S,

    pub metadata_mapper: MetadataMapper,
    pub payload_mapper: PayloadMapper,
}

impl<S: Sink> SinkTarget<S> {
    fn build_event(&self, mut event: cloudevents::Event) -> anyhow::Result<Option<Event>> {
        let meta = match self.metadata_mapper.map(&event)? {
            Some(meta) => meta,
            None => {
                return Ok(None);
            }
        };

        LAG.observe((Utc::now() - meta.timestamp).num_milliseconds() as f64);

        let message = self.payload_mapper.map(&meta, event.take_data())?;

        let Meta {
            id,
            timestamp,
            application,
            device,
            channel,
        } = meta;

        let thing = format!("{device}/{channel}");

        Ok(Some(Event {
            id,
            timestamp,
            application,
            thing,
            message,
        }))
    }
}

#[async_trait]
impl<S: Sink> Target for SinkTarget<S> {
    async fn event(&self, event: cloudevents::Event) -> anyhow::Result<()> {
        match self.build_event(event) {
            Ok(Some(event)) => {
                log::debug!("Injecting event: {event:?}");
                if let Err(err) = self.sink.publish(event).await {
                    log::error!("Failed to inject event: {err}, Exiting loop");
                    bail!("Failed to inject event: {err}")
                }
                EVENTS.with_label_values(&["ok"]).inc();
            }
            Ok(None) => {
                // got skipped
                EVENTS.with_label_values(&["skipped"]).inc();
            }
            Err(err) => {
                EVENTS.with_label_values(&["invalid"]).inc();
                log::info!("Unable to parse event: {err}, skipping...");
            }
        }

        Ok(())
    }
}

pub struct Injector<T: Target> {
    client: AsyncClient,
    events: EventLoop,
    target: T,
    topic: String,
}

macro_rules! close_or_break {
    ($client:expr) => {
        if let Err(err) = $client.try_disconnect() {
            log::error!("Failed to request disconnect: {err}");
            break;
        }
    };
}

impl<T: Target> Injector<T> {
    pub fn new(config: Config, target: T) -> anyhow::Result<Self> {
        let topic = config.topic;

        let opts = config.client.try_into()?;

        let (client, events) = AsyncClient::new(opts, 10);

        Ok(Self {
            topic,
            client,
            events,
            target,
        })
    }

    pub async fn run(mut self) -> anyhow::Result<()> {
        let mut perform_ack = true;
        loop {
            match self.events.poll().await {
                Err(err) => {
                    log::warn!("MQTT injector error: {err}");
                    // we keep going, polling the loop can fix things
                }
                Ok(rumqttc::Event::Incoming(Incoming::ConnAck(ack))) => {
                    log::info!("Connection open: {ack:?}");
                    if !ack.session_present {
                        log::info!("Subscribing to: {}", self.topic);
                        if let Err(err) = self.client.try_subscribe(&self.topic, QoS::AtLeastOnce) {
                            log::warn!("Failed to request subscription: {err}");
                            close_or_break!(self.client);
                        }
                    }
                }
                Ok(rumqttc::Event::Incoming(Incoming::SubAck(ack))) => {
                    log::info!("Subscription response: {ack:?}");
                    match ack.return_codes.as_slice() {
                        [SubscribeReasonCode::Success(QoS::AtLeastOnce)] => {
                            // all good
                            perform_ack = true;
                        }
                        [SubscribeReasonCode::Success(QoS::AtMostOnce)] => {
                            // got downgraded, we log and accept
                            log::warn!("Subscription got downgraded to QoS 0");
                            perform_ack = false;
                        }
                        ret => {
                            log::warn!("Unexpected subscription result: {:?}", ret);
                        }
                    }
                }
                Ok(rumqttc::Event::Incoming(Incoming::Publish(publish))) => {
                    if let Err(err) = self.handle_publish(&publish).await {
                        log::warn!("Failed to schedule message: {err}");
                        break;
                    }
                    if perform_ack {
                        if let Err(err) = self.client.try_ack(&publish) {
                            log::warn!("Failed to ack message: {err}");
                            close_or_break!(self.client);
                        }
                    }
                }
                Ok(rumqttc::Event::Incoming(Incoming::PubAck(ack))) => {
                    log::debug!("Got PUBACK for {ack:?}");
                }
                Ok(rumqttc::Event::Outgoing(_) | rumqttc::Event::Incoming(Incoming::PingResp)) => {
                    // ignore outgoing events and ping events
                }
                Ok(event) => {
                    log::info!("Unhandled event: {event:?}");
                }
            }
        }

        log::warn!("Exiting MQTT injector loop");

        Ok(())
    }

    #[instrument(skip_all, fields(topic=publish.topic, pkid=publish.pkid, payload_len=publish.payload.len()))]
    async fn handle_publish(&self, publish: &Publish) -> anyhow::Result<()> {
        let event: cloudevents::Event = serde_json::from_slice(&publish.payload)?;
        log::debug!("Cloud Event: {event:?}");

        self.target.event(event).await
    }
}
