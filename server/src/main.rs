mod injector;

#[macro_use]
extern crate diesel_migrations;

use actix_web::{middleware::Logger, App, HttpServer};
use anyhow::anyhow;
use drogue_doppelgaenger_core::config::kafka::KafkaProperties;
use drogue_doppelgaenger_core::processor::source::EventStream;
use drogue_doppelgaenger_core::processor::{source, Processor};
use drogue_doppelgaenger_core::service::Service;
use drogue_doppelgaenger_core::{
    app::run_main, config::ConfigFromEnv, notifier::kafka, service, storage::postgres,
};
use futures::{FutureExt, TryFutureExt};
use rdkafka::{
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    config::FromClientConfig,
};
use tokio::runtime::Handle;

embed_migrations!("../database-migration/migrations");

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Server {
    #[serde(default)]
    application: Option<String>,
    db: deadpool_postgres::Config,
    /// sink for change events
    kafka_sink: kafka::Config,
    /// source for change events
    kafka_source: kafka::Config,
    /// source for device events
    event_source: source::kafka::Config,
    /// optional injector
    #[serde(default)]
    injector: Option<injector::Config>,
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
    dotenv::dotenv()?;
    env_logger::builder().format_timestamp_millis().init();

    let server = Server::from_env()?;

    run_migrations(&server.db).await.unwrap();
    create_topic(
        KafkaProperties(server.kafka_sink.properties.clone()),
        server.kafka_sink.topic.clone(),
    )
    .await
    .unwrap();
    create_topic(
        KafkaProperties(server.event_source.properties.clone()),
        server.event_source.topic.clone(),
    )
    .await
    .unwrap();

    let service = service::Config {
        storage: postgres::Config {
            application: server.application.clone(),
            postgres: server.db,
        },
        notifier: server.kafka_sink,
    };
    let backend = drogue_doppelgaenger_backend::Config::<postgres::Storage, kafka::Notifier> {
        application: server.application,
        service: service.clone(),
        listener: server.kafka_source,
    };

    let configurator = drogue_doppelgaenger_backend::configure(backend)?;

    // prepare the http server

    let http = HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .configure(|ctx| configurator(ctx))
    })
    .bind("[::]:8080")?
    .run()
    .map_err(|err| anyhow!(err))
    .boxed_local();

    // prepare the incoming events processor

    let mut tasks = vec![];

    let (source, sink) = source::kafka::EventStream::new(server.event_source)?;

    if let Some(injector) = server.injector {
        log::info!("Running injector: {injector:?}");
        tasks.push(injector.run(sink.clone()).boxed_local());
    }

    let service = Service::new(service)?;
    let processor = Processor::new(service, source).run().boxed_local();

    tasks.extend([http, processor]);

    // run the main loop

    run_main(tasks).await
}
