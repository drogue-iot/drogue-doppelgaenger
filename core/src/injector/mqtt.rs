use crate::{
    injector::{
        metadata::{Meta, MetadataMapper},
        payload::PayloadMapper,
    },
    mqtt::MqttClient,
    processor::{sink::Sink, Event},
};
use anyhow::bail;
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
    pub async fn run<S: Sink>(
        self,
        sink: S,
        metadata_mapper: MetadataMapper,
        payload_mapper: PayloadMapper,
    ) -> anyhow::Result<()> {
        Injector::new(self, sink, metadata_mapper, payload_mapper)?
            .run()
            .await
    }
}

pub struct Injector<S: Sink> {
    client: AsyncClient,
    events: EventLoop,
    sink: S,
    topic: String,

    metadata_mapper: MetadataMapper,
    payload_mapper: PayloadMapper,
}

macro_rules! close_or_break {
    ($client:expr) => {
        if let Err(err) = $client.try_disconnect() {
            log::error!("Failed to request disconnect: {err}");
            break;
        }
    };
}

impl<S: Sink> Injector<S> {
    pub fn new(
        config: Config,
        sink: S,
        metadata_mapper: MetadataMapper,
        payload_mapper: PayloadMapper,
    ) -> anyhow::Result<Self> {
        let topic = config.topic;

        let opts = config.client.try_into()?;

        let (client, events) = AsyncClient::new(opts, 10);

        Ok(Self {
            topic,
            client,
            events,
            sink,
            metadata_mapper,
            payload_mapper,
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
                            // got downgraded, we log an accept
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

    #[instrument(skip_all, fields(payload_len=payload.len()), err)]
    fn build_event(&self, payload: &[u8]) -> anyhow::Result<Option<Event>> {
        let mut event: cloudevents::Event = serde_json::from_slice(payload)?;

        log::debug!("Cloud Event: {event:?}");

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

    #[instrument(skip_all, fields(topic=publish.topic, pkid=publish.pkid))]
    async fn handle_publish(&self, publish: &Publish) -> anyhow::Result<()> {
        match self.build_event(&publish.payload) {
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
