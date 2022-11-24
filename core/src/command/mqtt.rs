use crate::{command::Command, mqtt::MqttClient};
use async_trait::async_trait;
use drogue_bazaar::app::{Startup, StartupExt};
use rumqttc::{AsyncClient, ClientError, Event, EventLoop, Incoming, Outgoing, QoS};
use tracing::instrument;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct Config {
    #[serde(flatten)]
    pub client: MqttClient,
    #[serde(flatten, default)]
    pub mode: Option<Mode>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "mode")]
pub enum Mode {
    Drogue {
        // allow overriding the application
        #[serde(default)]
        application: Option<String>,
    },
}

impl Default for Mode {
    fn default() -> Self {
        Self::Drogue { application: None }
    }
}

impl Mode {
    pub fn build_topic(&self, application: String, device: String, channel: String) -> String {
        match self {
            Self::Drogue { application: a } => {
                format!(
                    "command/{}/{}/{}",
                    a.as_ref().unwrap_or(&application),
                    device,
                    channel
                )
            }
        }
    }
}

pub struct CommandSink {
    client: AsyncClient,
    mode: Mode,
}

#[async_trait]
impl super::CommandSink for CommandSink {
    type Error = ClientError;
    type Config = Config;

    fn from_config(startup: &mut dyn Startup, config: Self::Config) -> anyhow::Result<Self> {
        let opts = config.client.try_into()?;
        let mode = config.mode;

        let (client, event_loop) = AsyncClient::new(opts, 10);

        startup.spawn(Self::runner(event_loop));

        Ok(Self {
            client,
            mode: mode.unwrap_or_default(),
        })
    }

    #[instrument(skip_all, fields(
        application=command.application,
        device=command.device,
        channel=command.channel,
    ), err)]
    async fn send_command(&self, command: Command) -> Result<(), Self::Error> {
        let topic = self
            .mode
            .build_topic(command.application, command.device, command.channel);

        self.client
            .publish(topic, QoS::AtMostOnce, false, command.payload)
            .await
    }
}

impl CommandSink {
    async fn runner(mut event_loop: EventLoop) -> anyhow::Result<()> {
        loop {
            match event_loop.poll().await {
                Err(err) => {
                    log::info!("Connection error: {err}");
                    // keep going, as it will re-connect
                }
                Ok(Event::Incoming(Incoming::ConnAck(ack))) => {
                    log::info!("Connection opened: {ack:?}");
                }
                Ok(Event::Outgoing(Outgoing::Publish(id))) => {
                    log::debug!("Published: {id}");
                }
                Ok(Event::Incoming(Incoming::PubAck(ack))) => {
                    log::debug!("PubAck: {ack:?}");
                }
                Ok(Event::Incoming(Incoming::PingResp) | Event::Outgoing(Outgoing::PingReq)) => {
                    // ignore
                }
                Ok(event) => {
                    log::info!("Unexpected event: {event:?}");
                }
            }
        }
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use drogue_bazaar::core::config::ConfigFromEnv;
    use std::collections::HashMap;

    #[test]
    fn test_config() {
        let mut env = HashMap::<String, String>::new();
        env.insert("HOST".to_string(), "localhost".to_string());
        env.insert("PORT".to_string(), "8883".to_string());

        let config = Config::from_set(env).unwrap();

        assert_eq!(
            Config {
                client: MqttClient {
                    host: "localhost".to_string(),
                    port: 8883,
                    client_id: None,
                    username: None,
                    password: None,
                    clean_session: true,
                    disable_tls: false,
                    insecure: false,
                },
                mode: None,
            },
            config
        );
    }

    #[test]
    fn test_config_mode() {
        let mut env = HashMap::<String, String>::new();
        env.insert("HOST".to_string(), "localhost".to_string());
        env.insert("PORT".to_string(), "8883".to_string());

        env.insert("MODE".to_string(), "drogue".to_string());

        let config = Config::from_set(env).unwrap();

        assert_eq!(
            Config {
                client: MqttClient {
                    host: "localhost".to_string(),
                    port: 8883,
                    client_id: None,
                    username: None,
                    password: None,
                    clean_session: true,
                    disable_tls: false,
                    insecure: false,
                },
                mode: Some(Mode::Drogue { application: None })
            },
            config
        );
    }

    #[test]
    fn test_config_mode_options() {
        let mut env = HashMap::<String, String>::new();
        env.insert("HOST".to_string(), "localhost".to_string());
        env.insert("PORT".to_string(), "8883".to_string());

        env.insert("MODE".to_string(), "drogue".to_string());
        env.insert("APPLICATION".to_string(), "app".to_string());

        let config = Config::from_set(env).unwrap();

        assert_eq!(
            Config {
                client: MqttClient {
                    host: "localhost".to_string(),
                    port: 8883,
                    client_id: None,
                    username: None,
                    password: None,
                    clean_session: true,
                    disable_tls: false,
                    insecure: false,
                },
                mode: Some(Mode::Drogue {
                    application: Some("app".to_string())
                })
            },
            config
        );
    }
}
