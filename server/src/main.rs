#[macro_use]
extern crate diesel_migrations;

use actix_web::middleware::Logger;
use actix_web::{App, HttpServer};
use anyhow::anyhow;
use drogue_doppelgaenger_core::notifier::kafka;
use drogue_doppelgaenger_core::storage::postgres;
use drogue_doppelgaenger_core::{app::run_main, config::ConfigFromEnv, service};
use futures::{FutureExt, TryFutureExt};
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv()?;
    env_logger::builder().format_timestamp_millis().init();

    let server = Server::from_env()?;

    let backend = drogue_doppelgaenger_backend::Config::<postgres::Storage, kafka::Notifier> {
        application: server.application.clone(),
        service: service::Config {
            storage: postgres::Config {
                application: server.application,
                postgres: server.db.clone(),
            },
            notifier: server.kafka_sink,
        },
        listener: server.kafka_source,
    };

    run_migrations(&server.db).await.unwrap();

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
