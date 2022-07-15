#[macro_use]
extern crate diesel_migrations;

use actix_web::{middleware::Logger, App, HttpServer};
use anyhow::anyhow;
use drogue_doppelgaenger_core::{
    app::run_main, config::ConfigFromEnv, notifier::kafka, service, storage::postgres,
};
use futures::{FutureExt, TryFutureExt};
use rdkafka::admin::{AdminOptions, NewTopic, TopicReplication};
use rdkafka::{admin::AdminClient, config::FromClientConfig};
use tokio::runtime::Handle;

embed_migrations!("../database-migration/migrations");

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Server {
    #[serde(default)]
    application: Option<String>,
    db: deadpool_postgres::Config,
    kafka_sink: kafka::Config,
    kafka_source: kafka::Config,
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

async fn create_topic(config: &kafka::Config) -> anyhow::Result<()> {
    let topic = config.topic.clone();
    let config: rdkafka::ClientConfig = config.clone().into();
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
    dotenv::dotenv()?;
    env_logger::builder().format_timestamp_millis().init();

    let server = Server::from_env()?;

    run_migrations(&server.db).await.unwrap();
    create_topic(&server.kafka_sink).await.unwrap();

    let backend = drogue_doppelgaenger_backend::Config::<postgres::Storage, kafka::Notifier> {
        application: server.application.clone(),
        service: service::Config {
            storage: postgres::Config {
                application: server.application,
                postgres: server.db,
            },
            notifier: server.kafka_sink,
        },
        listener: server.kafka_source,
    };

    let configurator = drogue_doppelgaenger_backend::configure(backend)?;

    let http = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .configure(|ctx| configurator(ctx))
    })
    .bind("[::]:8080")?
    .run()
    .map_err(|err| anyhow!(err))
    .boxed_local();

    run_main([http]).await
}
