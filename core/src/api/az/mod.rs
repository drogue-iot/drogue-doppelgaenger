mod types;

use anyhow::anyhow;
use std::collections::BTreeMap;
use std::str::from_utf8;
pub use types::*;

use crate::{
    command::{Command, CommandSink},
    events::DataExt,
    injector::{mqtt::Target, SourceConfig},
    processor::{sink::Sink, Event, Message},
    service::{Id, Service},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use cloudevents::{event::ExtensionValue, AttributesReader};
use lazy_static::lazy_static;
use prometheus::{register_histogram, register_int_counter_vec, Histogram, IntCounterVec};
use serde_json::Value;
use tracing::instrument;

lazy_static! {
    static ref EVENTS: IntCounterVec = register_int_counter_vec!(
        "az_events",
        "Number of events processed by Azure API",
        &["result"]
    )
    .unwrap();
    static ref LAG: Histogram =
        register_histogram!("az_lag", "Lag in ms for the injector").unwrap();
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    pub source: SourceConfig,

    #[serde(default, flatten)]
    pub twin: TwinConfig,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct TwinConfig {
    #[serde(default)]
    pub application: Option<String>,
}

impl Config {
    pub async fn run<S, C, Svc>(self, sink: S, command: C, service: Svc) -> anyhow::Result<()>
    where
        S: Sink,
        C: CommandSink,
        Svc: Service + Sync + Send,
        Svc::Error: Sync + Send + 'static,
    {
        Twin::new(self, sink, command, service)?.run().await
    }
}

pub struct Twin<S: Sink, C: CommandSink, Svc: Service> {
    config: Config,

    command: C,
    sink: S,
    service: Svc,
}

struct TwinTarget<S: Sink, C: CommandSink, Svc: Service> {
    command: C,
    sink: S,
    service: Svc,
    config: TwinConfig,
}

impl<S, C, Svc> Twin<S, C, Svc>
where
    S: Sink,
    C: CommandSink,
    Svc: Service + Sync + Send,
    Svc::Error: Sync + Send + 'static,
{
    pub fn new(config: Config, sink: S, command: C, service: Svc) -> anyhow::Result<Self> {
        Ok(Self {
            config,
            sink,
            command,
            service,
        })
    }

    pub async fn run(self) -> anyhow::Result<()> {
        self.config
            .source
            .run(TwinTarget {
                sink: self.sink,
                command: self.command,
                config: self.config.twin,
                service: self.service,
            })
            .await
    }
}

#[async_trait]
impl<S, C, Svc> Target for TwinTarget<S, C, Svc>
where
    S: Sink,
    C: CommandSink,
    Svc: Service + Sync + Send,
    Svc::Error: Sync + Send + 'static,
{
    #[instrument(
        skip_all, fields(
            id=event.id(),
            time=event.time().map(|s|s.timestamp_millis()),
            subject=event.subject()),
        ret, err)]
    async fn event(&self, mut event: cloudevents::Event) -> anyhow::Result<()> {
        let timestamp = match event.time() {
            Some(time) => time,
            _ => return Ok(()),
        };
        LAG.observe((Utc::now() - *timestamp).num_milliseconds() as f64);

        let source_application = match event.extension("application") {
            Some(ExtensionValue::String(application)) => application.to_string(),
            _ => return Ok(()),
        };

        let application = self
            .config
            .application
            .clone()
            .unwrap_or_else(|| source_application.clone());

        let device = match event.extension("device") {
            Some(ExtensionValue::String(device)) => device.to_string(),
            _ => return Ok(()),
        };
        let channel = match event.subject() {
            Some(subject) => subject,
            _ => return Ok(()),
        };
        let rid = event.extension("$rid").map(|s| s.to_string());

        log::info!("Application: {application} (source: {source_application}), device: {device}, channel: {channel}");
        log::debug!("Payload: {:?}", event.data());

        let rid = match rid {
            Some(rid) => rid,
            None => return Ok(()),
        };

        let ctx = RequestContext {
            id: event.id().to_string(),
            timestamp: timestamp.clone(),
            source_application,
            application,
            device,
            rid,
        };

        match channel {
            "$iothub/twin/GET" => {
                let result = self.get(&ctx).await;
                self.send_result(ctx, result).await?;
            }
            "$iothub/twin/PATCH/properties/reported" => {
                let result = self
                    .patch(
                        &ctx,
                        event
                            .take_as_json()?
                            .ok_or_else(|| anyhow!("Missing request payload"))?,
                    )
                    .await;
                self.send_result(ctx, result).await?;
            }
            _ => {}
        }

        Ok(())
    }
}

#[derive(Debug)]
struct RequestContext {
    /// Source event ID
    id: String,
    /// Source event timestamp
    timestamp: DateTime<Utc>,
    /// the application name in the source
    source_application: String,
    /// the application name in doppelgaenger
    application: String,
    /// the device name
    device: String,
    /// The Azure request ID
    rid: String,
}

#[derive(Debug)]
pub struct Response {
    pub code: u16,
    pub payload: Vec<u8>,
}

impl<S, C, Svc> TwinTarget<S, C, Svc>
where
    S: Sink,
    C: CommandSink,
    Svc: Service,
    Svc::Error: Send + Sync + 'static,
{
    /// Take a result and send a response.
    async fn send_result(
        &self,
        ctx: RequestContext,
        result: anyhow::Result<Response>,
    ) -> Result<(), C::Error> {
        match result {
            Ok(response) => {
                log::debug!(
                    "Response ({ctx:?}): {}: {:?}",
                    response.code,
                    from_utf8(&response.payload)
                );
                self.send_response(ctx, response.code, response.payload)
                    .await
            }
            Err(err) => {
                log::info!("Failed to process: {err}");
                self.send_response(ctx, 500, vec![]).await
            }
        }
    }

    /// Send a response in the form of an azure twin response.
    async fn send_response(
        &self,
        ctx: RequestContext,
        code: u16,
        payload: Vec<u8>,
    ) -> Result<(), C::Error> {
        self.command
            .send_command(Command {
                application: ctx.source_application.to_string(),
                device: ctx.device.to_string(),
                channel: format!("$iothub/twin/res/{}/?$rid={}", code, ctx.rid),
                payload,
            })
            .await
    }

    /// Get operation.
    async fn get(&self, ctx: &RequestContext) -> anyhow::Result<Response> {
        match self
            .service
            .get(&Id {
                application: ctx.application.to_string(),
                thing: ctx.device.to_string(),
            })
            .await?
        {
            None => Ok(Response {
                code: 404,
                payload: vec![],
            }),
            Some(thing) => Ok(Response {
                code: 200,
                payload: serde_json::to_vec(&TwinProperties::from(thing))?,
            }),
        }
    }

    async fn patch(
        &self,
        ctx: &RequestContext,
        properties: BTreeMap<String, Value>,
    ) -> anyhow::Result<Response> {
        self.sink
            .publish(Event {
                id: ctx.id.clone(),
                timestamp: ctx.timestamp,
                application: ctx.application.clone(),
                thing: ctx.device.clone(),
                message: Message::ReportState {
                    state: properties,
                    partial: true,
                },
            })
            .await?;

        Ok(Response {
            code: 200,
            payload: vec![],
        })
    }
}
