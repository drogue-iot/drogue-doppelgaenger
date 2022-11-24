use anyhow::{bail, Context};
use rumqttc::{MqttOptions, TlsConfiguration, Transport};
use rustls::{
    client::{NoClientSessionStorage, ServerCertVerified, ServerCertVerifier},
    Certificate, ClientConfig, Error, ServerName,
};
use std::{sync::Arc, time::SystemTime};
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
    #[serde(default)]
    pub insecure: bool,
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
            opts.set_transport(Transport::Tls(setup_tls(config.insecure)?));
        }

        Ok(opts)
    }
}

pub struct InsecureServerCertVerifier;

impl ServerCertVerifier for InsecureServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }
}

/// Setup TLS with RusTLS and system certificates.
fn setup_tls(insecure: bool) -> anyhow::Result<TlsConfiguration> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_native_certs::load_native_certs().context("could not load platform certs")? {
        roots.add(&rustls::Certificate(cert.0))?;
    }

    let mut client_config = ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(roots)
        .with_no_client_auth();

    if insecure {
        log::warn!("Disabling TLS validation. Do not use this in production!");
        client_config
            .dangerous()
            .set_certificate_verifier(Arc::new(InsecureServerCertVerifier));
    }

    client_config.session_storage = Arc::new(NoClientSessionStorage {});

    Ok(TlsConfiguration::Rustls(Arc::new(client_config)))
}
