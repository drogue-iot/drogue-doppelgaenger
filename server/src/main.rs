mod injector;

#[macro_use]
extern crate diesel_migrations;

use actix_web::{middleware::Logger, App, HttpServer};
use anyhow::anyhow;
use drogue_doppelgaenger_core::{
    app::run_main,
    config::{kafka::KafkaProperties, ConfigFromEnv},
    notifier,
    processor::{
        sink::{self, Sink},
        source::{self, Source},
        Processor,
    },
    service::{self, Service},
    storage::postgres,
    waker::{self},
};
use futures::{FutureExt, TryFutureExt};
use rdkafka::{
    admin::{AdminClient, AdminOptions, NewTopic, TopicReplication},
    config::FromClientConfig,
};
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

    /// optional injector
    #[serde(default)]
    injector: Option<injector::Config>,

    #[serde(with = "humantime_serde")]
    #[serde(default = "waker::postgres::default::check_duration")]
    check_duration: Duration,
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

    let service = service::Config {
        storage: postgres::Config {
            application: server.application.clone(),
            postgres: server.db.clone(),
        },
        notifier: server.notifier_sink,
        sink: server.event_sink.clone(),
    };
    let backend = drogue_doppelgaenger_backend::Config::<
        postgres::Storage,
        notifier::kafka::Notifier,
        sink::kafka::Sink,
    > {
        application: server.application.clone(),
        service: service.clone(),
        listener: server.notifier_source,
    };

    let (configurator, runner) = drogue_doppelgaenger_backend::configure(backend)?;

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

    let mut tasks = vec![runner];

    let sink = sink::kafka::Sink::from_config(server.event_sink.clone())?;
    let source = source::kafka::Source::from_config(server.event_source)?;

    if let Some(injector) = server.injector.filter(|injector| !injector.disabled) {
        log::info!("Running injector: {injector:?}");
        tasks.push(injector.run(sink).boxed_local());
    }

    let service = Service::from_config(service)?;

    let processor = Processor::new(service, source).run().boxed_local();

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

    tasks.extend([http, processor, waker]);

    // run the main loop

    run_main(tasks).await
}
