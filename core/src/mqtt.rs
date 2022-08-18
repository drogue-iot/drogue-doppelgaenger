use anyhow::{bail, Context};
use rumqttc::{MqttOptions, TlsConfiguration, Transport};
use rustls::client::NoClientSessionStorage;
use rustls::ClientConfig;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
pub struct MqttClient {
    pub host: String,
    pub port: u16,

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

impl TryFrom<MqttClient> for MqttOptions {
    type Error = anyhow::Error;

    fn try_from(config: MqttClient) -> Result<Self, Self::Error> {
        let mut opts = MqttOptions::new(
            config
                .client_id
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            config.host,
            config.port,
        );

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

        Ok(opts)
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
