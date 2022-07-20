use crate::injector::{
    metadata::{Meta, MetadataMapper},
    payload::PayloadMapper,
};
use anyhow::{bail, Context};
use drogue_doppelgaenger_core::processor::{source::Sink, Event};
use rumqttc::{
    AsyncClient, EventLoop, Incoming, MqttOptions, QoS, SubscribeReasonCode, TlsConfiguration,
    Transport,
};
use rustls::{client::NoClientSessionStorage, ClientConfig};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub topic: String,

    #[serde(default)]
    pub client_id: Option<String>,

    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,

    #[serde(default = "default::clean_session")]
    pub clean_session: bool,
    #[serde(default)]
    pub disable_tls: bool,
}

mod default {
    pub const fn clean_session() -> bool {
        true
    }
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
        let mut opts = MqttOptions::new(
            config
                .client_id
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            config.host,
            config.port,
        );

        let topic = config.topic;

        match (config.username, config.password) {
            (Some(username), Some(password)) => {
                opts.set_credentials(username, password);
            }
            (None, None) => {}
            (Some(_), None) => bail!("Unsupported MQTT configuration: username but no password"),
            (None, Some(_)) => bail!("Unsupported MQTT configuration: password but no username"),
        }

        opts.set_manual_acks(true)
            .set_clean_session(config.clean_session);

        if !config.disable_tls {
            opts.set_transport(Transport::Tls(setup_tls()?));
        }

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
                    match self.build_event(&publish.payload) {
                        Ok(Some(event)) => {
                            log::debug!("Injecting event: {event:?}");
                            if let Err(err) = self.sink.publish(event).await {
                                log::error!("Failed to inject event: {err}, Exiting loop");
                            }
                        }
                        Ok(None) => {
                            // got skipped
                        }
                        Err(err) => log::info!("Unable to parse event: {err}, skipping..."),
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

    fn build_event(&self, payload: &[u8]) -> anyhow::Result<Option<Event>> {
        let mut event: cloudevents::Event = serde_json::from_slice(payload)?;

        log::debug!("Cloud Event: {event:?}");

        let Meta {
            id,
            timestamp,
            application,
            device,
        } = match self.metadata_mapper.map(&event)? {
            Some(meta) => meta,
            None => {
                return Ok(None);
            }
        };

        let message = self.payload_mapper.map(event.take_data())?;

        Ok(Some(Event {
            id,
            timestamp,
            application,
            device,
            message,
        }))
    }
}

/// Setup TLS with RusTLS and system certificates.
fn setup_tls() -> anyhow::Result<TlsConfiguration> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().context("could not load platform certs")? {
        roots.add(&rustls::Certificate(cert.0))?;
    }

    let mut client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();

    client_config.session_storage = Arc::new(NoClientSessionStorage {});

    Ok(TlsConfiguration::Rustls(Arc::new(client_config)))
}
