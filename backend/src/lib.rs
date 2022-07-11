mod endpoints;

use actix_web::web::ServiceConfig;
use actix_web::{web, App, HttpServer};
use anyhow::anyhow;
use drogue_doppelgaenger_common::app::run_main;
use drogue_doppelgaenger_common::service::{redis, Service};
use futures::future::FutureExt;
use futures::TryFutureExt;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config<S: Service> {
    instance: String,

    service: S::Config,
}

fn configure<S: Service>(
    config: Config<S>,
) -> anyhow::Result<impl Fn(&mut ServiceConfig) + Send + Sync + Clone> {
    let service = web::Data::new(S::new(config.service.clone())?);
    let config = web::Data::new(config);
    Ok(move |ctx: &mut ServiceConfig| {
        ctx.app_data(config.clone());
        ctx.app_data(service.clone());
        ctx.service(
            web::resource("/api/v1alpha1/things/{application}/{thing}")
                .route(web::get().to(endpoints::things_get::<S>))
                .route(web::delete().to(endpoints::things_delete::<S>)),
        );
        ctx.service(
            web::resource("/api/v1alpha1/things")
                .route(web::post().to(endpoints::things_create::<S>))
                .route(web::put().to(endpoints::things_update::<S>)),
        );
    })
}

pub async fn run(config: Config<redis::Service>) -> anyhow::Result<()> {
    let configurator = configure(config)?;

    let http = HttpServer::new(move || App::new().configure(|ctx| configurator(ctx)))
        .bind("0.0.0.0:8080")?
        .run()
        .map_err(|err| anyhow!(err))
        .boxed_local();

    run_main([http]).await
}
