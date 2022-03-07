mod metrics;
mod processor;

use actix_web::{web, HttpResponse, HttpServer};
use actix_web_prom::PrometheusMetricsBuilder;
use anyhow::anyhow;
use config::{Config, Environment};
use prometheus::Registry;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use tokio::select;

#[derive(Clone, Debug, Deserialize)]
pub struct ApplicationConfig {
    pub kafka: KafkaClient,
    pub mongodb: MongoDbClient,
    #[serde(default)]
    pub metrics: MetricsConfig,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct MetricsConfig {
    #[serde(default)]
    pub bind_addr: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct KafkaClient {
    pub bootstrap_servers: String,
    #[serde(default)]
    pub properties: HashMap<String, String>,
    pub topic: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MongoDbClient {
    pub url: String,
    pub database: String,
}

async fn start_metrics(config: MetricsConfig, registry: Registry) -> anyhow::Result<()> {
    let prometheus = PrometheusMetricsBuilder::new("health")
        .registry(registry)
        .endpoint("/metrics")
        .build()
        .map_err(|err| anyhow!("Failed to build prometheus metrics: {err}"))?;

    HttpServer::new(move || {
        use actix_web::App;

        App::new()
            .route(
                "/",
                web::get().to(|| async { HttpResponse::Ok().json(json!({"success": true})) }),
            )
            .wrap(prometheus.clone())
    })
    .bind(
        config
            .bind_addr
            .clone()
            .unwrap_or_else(|| "127.0.0.1:8081".into()),
    )?
    .workers(1)
    .run()
    .await?;

    log::info!("Metrics runner exiting...");

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let config: ApplicationConfig = Config::builder()
        .add_source(Environment::default().separator("__"))
        .build()?
        .try_deserialize()?;

    log::info!("Configuration: {:#?}", config);

    let metrics = config.metrics.clone();
    let app = processor::Processor::new(config).await?;

    let dashboard_data = &mut metrics::Metrics::new();

    // run

    select! {
        _ = app.run(dashboard_data) => {},
        _ = start_metrics(metrics, prometheus::default_registry().clone()) => {},
    }

    // done

    log::warn!("Kafka stream finished. Exiting...");

    Ok(())
}
