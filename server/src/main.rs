mod keycloak;

#[macro_use]
extern crate diesel_migrations;

use crate::keycloak::SERVICE_CLIENT_SECRET;
use drogue_bazaar::auth::openid::{AuthenticatorClientConfig, AuthenticatorGlobalConfig};
use drogue_bazaar::{
    actix::http::{CorsBuilder, HttpBuilder, HttpConfig},
    app::Startup,
    auth::openid::AuthenticatorConfig,
    core::SpawnerExt,
    runtime,
};
use drogue_doppelgaenger_core::{
    command,
    config::kafka::KafkaProperties,
    injector, notifier,
    processor::{
        sink::{self, Sink},
        source::{self, Source},
        Processor,
    },
    service::{self, Service},
    storage::postgres,
    waker::{self},
};
use futures::FutureExt;
use rdkafka::{
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    config::FromClientConfig,
};
use std::collections::HashMap;
use std::time::Duration;
use tokio::runtime::Handle;

embed_migrations!("../database-migration/migrations");

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Server {
    #[serde(default)]
    application: Option<String>,
    db: deadpool_postgres::Config,

    /// sink for change events
    notifier_sink: notifier::kafka::Config,
    /// source for change events
    notifier_source: notifier::kafka::Config,

    // sink for events
    event_sink: sink::kafka::Config,
    // source for events
    event_source: source::kafka::Config,

    // sink for commands
    command_sink: command::mqtt::Config,

    /// optional injector
    #[serde(default)]
    injector: Option<injector::Config>,

    #[serde(with = "humantime_serde")]
    #[serde(default = "waker::postgres::default::check_duration")]
    check_duration: Duration,

    #[serde(default)]
    http: HttpConfig,

    #[serde(default)]
    oauth: Option<AuthenticatorConfig>,

    #[serde(default)]
    keycloak: keycloak::Keycloak,
}

mod default {
    #[allow(unused)]
    pub fn application() -> String {
        "default".into()
    }
}

pub async fn run_migrations(db: &deadpool_postgres::Config) -> anyhow::Result<()> {
    use diesel::Connection;
    log::info!("Migrating database schema...");
    let database_url = format!(
        "postgres://{}:{}@{}:{}/{}",
        db.user.as_ref().unwrap(),
        db.password.as_ref().unwrap(),
        db.host.as_ref().unwrap(),
        db.port.unwrap(),
        db.dbname.as_ref().unwrap()
    );

    Handle::current()
        .spawn_blocking(move || {
            let connection = diesel::PgConnection::establish(&database_url)
                .unwrap_or_else(|_| panic!("Error connecting to {}", database_url));

            embedded_migrations::run_with_output(&connection, &mut std::io::stdout()).unwrap();
            log::info!("Migrating database schema... done!");
        })
        .await?;

    Ok(())
}

async fn create_topic(config: KafkaProperties, topic: String) -> anyhow::Result<()> {
    let config: rdkafka::ClientConfig = config.into();
    let client = AdminClient::from_config(&config)?;

    client
        .create_topics(
            &[NewTopic::new(&topic, 1, TopicReplication::Fixed(1))],
            &AdminOptions::new(),
        )
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime!(drogue_doppelgaenger_core::PROJECT).exec(run).await
}

async fn run(server: Server, startup: &mut dyn Startup) -> anyhow::Result<()> {
    run_migrations(&server.db).await.unwrap();
    create_topic(
        KafkaProperties(server.notifier_sink.properties.clone()),
        server.notifier_sink.topic.clone(),
    )
    .await
    .unwrap();
    create_topic(
        KafkaProperties(server.event_sink.properties.clone()),
        server.event_sink.topic.clone(),
    )
    .await
    .unwrap();

    let oauth = if !server.keycloak.disabled {
        keycloak::configure_keycloak(&server.keycloak)
            .await
            .map_err(|err| {
                log::error!("Failed to setup keycloak: {err}");
                err
            })
            .expect("Set up keycloak");
        log::info!("OAuth: {:?}", server.oauth);
        server.oauth.or_else(|| {
            let mut clients = HashMap::new();
            clients.insert(
                "api".to_string(),
                AuthenticatorClientConfig {
                    client_id: "api".to_string(),
                    client_secret: "".to_string(),
                    scopes: "openid profile".to_string(),
                    issuer_url: None,
                    tls_insecure: None,
                    tls_ca_certificates: None,
                },
            );
            clients.insert(
                "services".to_string(),
                AuthenticatorClientConfig {
                    client_id: "services".to_string(),
                    client_secret: SERVICE_CLIENT_SECRET.to_string(),
                    scopes: "openid profile".to_string(),
                    issuer_url: None,
                    tls_insecure: None,
                    tls_ca_certificates: None,
                },
            );
            Some(AuthenticatorConfig {
                disabled: false,
                global: AuthenticatorGlobalConfig {
                    issuer_url: Some(format!(
                        "{}/realms/{}",
                        server.keycloak.url, server.keycloak.realm
                    )),
                    redirect_url: None,
                    tls_insecure: true,
                    tls_ca_certificates: Default::default(),
                },
                clients,
            })
        })
    } else {
        server.oauth
    }
    .unwrap_or_else(|| AuthenticatorConfig {
        disabled: true,
        global: AuthenticatorGlobalConfig {
            issuer_url: None,
            redirect_url: None,
            tls_insecure: false,
            tls_ca_certificates: Default::default(),
        },
        clients: Default::default(),
    });

    let service = service::Config {
        storage: postgres::Config {
            application: server.application.clone(),
            postgres: server.db.clone(),
        },
        notifier: server.notifier_sink,
        sink: server.event_sink.clone(),
        command_sink: server.command_sink,
    };
    let backend = drogue_doppelgaenger_backend::Config::<
        postgres::Storage,
        notifier::kafka::Notifier,
        sink::kafka::Sink,
        command::mqtt::CommandSink,
    > {
        application: server.application.clone(),
        service: service.clone(),
        listener: server.notifier_source,
        oauth,
        user_auth: None,
    };

    let configurator = drogue_doppelgaenger_backend::configure(startup, backend).await?;

    // prepare the http server

    HttpBuilder::new(
        server.http.clone(),
        Some(startup.runtime_config()),
        move |config| {
            config.configure(|ctx| configurator(ctx));
        },
    )
    .cors(CorsBuilder::Permissive)
    .start(startup)?;

    // prepare the incoming events processor

    let sink = sink::kafka::Sink::from_config(server.event_sink.clone())?;
    let source = source::kafka::Source::from_config(server.event_source)?;

    if let Some(injector) = server.injector.filter(|injector| !injector.disabled) {
        log::info!("Running injector: {injector:?}");
        startup.spawn(injector.run(sink).boxed_local());
    }

    let service = Service::from_config(startup, service)?;

    let processor = Processor::new(service, source).run().boxed();

    let waker = waker::Processor::from_config(waker::Config::<
        waker::postgres::Waker,
        sink::kafka::Sink,
    > {
        waker: waker::postgres::Config {
            application: server.application,
            postgres: server.db,
            check_period: server.check_duration,
        },
        sink: server.event_sink,
    })?
    .run()
    .boxed_local();

    startup.spawn_iter([processor, waker]);

    Ok(())
}
