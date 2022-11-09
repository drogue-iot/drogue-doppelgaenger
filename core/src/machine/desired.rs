use super::deno;
use crate::{
    command,
    machine::{
        deno::{DenoOptions, Execution, Json},
        recon::ScriptAction,
    },
    model::{
        self, Code, CommandEncoding, CommandMode, Internal, Thing, Waker, WakerExt, WakerReason,
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use time::Duration;
use tokio::time::Instant;

#[derive(Default)]
pub struct CommandBuilder {
    // the channel commands
    channels: BTreeMap<String, BTreeMap<String, BTreeMap<String, Value>>>,
    // the raw commands
    commands: Vec<command::Command>,
}

impl CommandBuilder {
    pub fn into_commands(
        mut self,
        application: &str,
    ) -> Result<Vec<command::Command>, serde_json::Error> {
        for (device, channels) in self.channels {
            for (channel, values) in channels {
                self.commands.push(command::Command {
                    application: application.to_string(),
                    device: device.clone(),
                    channel,
                    payload: serde_json::to_vec(&values)?,
                })
            }
        }

        Ok(self.commands)
    }

    pub fn push_channel(&mut self, device: String, channel: String, name: String, value: Value) {
        self.channels
            .entry(device)
            .or_default()
            .entry(channel)
            .or_default()
            .insert(name, value);
    }

    pub fn push_command(&mut self, command: command::Command) {
        self.commands.push(command);
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Command {
    #[serde(default)]
    pub device: Option<String>,
    pub channel: String,
    #[serde(default)]
    pub payload: Payload,
}

#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(untagged)]
pub enum Payload {
    String(String),
    Json(Value),
    Binary(Vec<u8>),
    #[default]
    Empty,
}

impl TryFrom<Payload> for Vec<u8> {
    type Error = serde_json::Error;

    fn try_from(value: Payload) -> Result<Self, Self::Error> {
        Ok(match value {
            Payload::Empty => Vec::new(),
            Payload::Binary(payload) => payload,
            Payload::String(payload) => payload.into_bytes(),
            Payload::Json(payload) => serde_json::to_vec(&payload)?,
        })
    }
}

pub struct Context<'r> {
    pub new_thing: Arc<Thing<Internal>>,
    pub deadline: Instant,
    pub waker: &'r mut Waker,
    pub commands: &'r mut CommandBuilder,
}

pub struct FeatureContext<'r> {
    pub value: Value,
    pub last_attempt: &'r mut Option<DateTime<Utc>>,
    pub name: &'r str,
}

#[async_trait]
pub trait DesiredReconciler {
    type Error;

    async fn reconcile<'r>(
        &self,
        context: &mut Context<'r>,
        input: FeatureContext<'r>,
    ) -> Result<(), Self::Error>;
}

#[async_trait]
impl DesiredReconciler for Code {
    type Error = anyhow::Error;

    async fn reconcile<'r>(
        &self,
        context: &mut Context<'r>,
        feature: FeatureContext<'r>,
    ) -> Result<(), Self::Error> {
        match self {
            Code::JavaScript(code) => {
                #[derive(Clone, serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                pub struct Input {
                    value: Value,
                    action: ScriptAction,
                    #[serde(skip_serializing_if = "Option::is_none")]
                    last_attempt: Option<DateTime<Utc>>,
                }

                #[derive(Clone, Default, serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                pub struct Output {
                    #[serde(
                        default,
                        with = "deno::duration",
                        skip_serializing_if = "Vec::is_empty"
                    )]
                    waker: Option<Duration>,

                    #[serde(default, skip_serializing_if = "Vec::is_empty")]
                    commands: Vec<Command>,
                }

                let name = feature.name.to_string();

                let input = Input {
                    value: feature.value,
                    last_attempt: *feature.last_attempt,
                    action: ScriptAction::DesiredReconciliation,
                };

                let exec = Execution::new(
                    name,
                    code,
                    DenoOptions {
                        deadline: context.deadline,
                    },
                );

                let result = exec.run::<_, Json<Output>, ()>(Json(input)).await?;

                let Output { waker, commands } = result.output.0;

                if let Some(waker) = waker {
                    context.waker.wakeup(waker, WakerReason::Reconcile);
                }

                if !commands.is_empty() {
                    // tick "last attempt" only when we make an attempt (by sending commands)
                    *feature.last_attempt = Some(Utc::now());
                    // FIXME: find a way to support "channel" aggregation too
                    for command in commands {
                        context.commands.push_command(command::Command {
                            application: context.new_thing.metadata.application.clone(),
                            device: command
                                .device
                                .unwrap_or_else(|| context.new_thing.metadata.name.clone()),
                            channel: command.channel,
                            payload: vec![],
                        });
                    }
                }

                Ok(())
            }
        }
    }
}

#[async_trait]
impl DesiredReconciler for model::Command {
    type Error = anyhow::Error;

    async fn reconcile<'r>(
        &self,
        context: &mut Context<'r>,
        input: FeatureContext<'r>,
    ) -> Result<(), Self::Error> {
        // FIXME: we need some kind of "is connected" handling
        // FIXME: we need to combine this with the "command timeout" polling

        let period = Duration::from_std(self.period)?;

        match input.last_attempt {
            Some(last_attempt) => {
                // check due time
                let due = *last_attempt + period;
                if due > Utc::now() {
                    if matches!(self.mode, CommandMode::Active) {
                        context.waker.wakeup_at(due, WakerReason::Reconcile);
                    }
                    return Ok(());
                }
            }
            None => {
                // do it now
            }
        }

        // last_attempt = now
        *input.last_attempt = Some(Utc::now());
        if matches!(self.mode, CommandMode::Active) {
            context.waker.wakeup(period, WakerReason::Reconcile);
        }

        let encoding = self.encoding.clone().unwrap_or_else(|| {
            if let Some(channel) = context
                .new_thing
                .metadata
                .annotations
                .get("drogue.io/channel")
            {
                CommandEncoding::Channel(channel.to_string())
            } else {
                CommandEncoding::Raw
            }
        });

        let device = context
            .new_thing
            .metadata
            .annotations
            .get("drogue.io/device")
            .unwrap_or(&context.new_thing.metadata.name)
            .clone();

        match encoding {
            CommandEncoding::Remap { device, channel } => {
                context
                    .commands
                    .push_channel(device, channel, input.name.to_string(), input.value);
            }
            CommandEncoding::Channel(channel) => {
                context
                    .commands
                    .push_channel(device, channel, input.name.to_string(), input.value);
            }
            CommandEncoding::Raw => {
                context.commands.push_command(command::Command {
                    application: context.new_thing.metadata.application.clone(),
                    device,
                    channel: input.name.to_string(),
                    payload: serde_json::to_vec(&input.value)?,
                });
            }
        }

        Ok(())
    }
}
