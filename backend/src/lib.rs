mod endpoints;
mod notifier;
mod utils;

use actix_web::{guard, web, App, HttpServer};
use anyhow::anyhow;
use drogue_doppelgaenger_core::{
    app::{run_main, Spawner},
    command::{mqtt, CommandSink},
    listener::KafkaSource,
    notifier::{kafka, Notifier},
    processor::sink::{self, Sink},
    service::{self, Service},
    storage::{postgres, Storage},
};
use futures::{FutureExt, TryFutureExt};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> {
    pub application: Option<String>,
    // serde(bound) required as S isn't serializable: https://github.com/serde-rs/serde/issues/1296
    #[serde(bound = "")]
    pub service: service::Config<S, N, Si, Cmd>,

    pub listener: kafka::Config,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub application: Option<String>,
}

pub fn configure<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    spawner: &mut dyn Spawner,
    config: Config<S, N, Si, Cmd>,
) -> anyhow::Result<impl Fn(&mut web::ServiceConfig) + Send + Sync + Clone> {
    let service = Service::from_config(spawner, config.service)?;
    let service = web::Data::new(service);

    let source = KafkaSource::new(spawner, config.listener)?;
    let source = web::Data::new(source);

    let instance = web::Data::new(Instance {
        application: config.application,
    });

    Ok(move |ctx: &mut web::ServiceConfig| {
        ctx.app_data(service.clone());
        ctx.app_data(instance.clone());
        ctx.app_data(source.clone());
        ctx.service(
            web::resource("/api/v1alpha1/things")
                .route(web::post().to(endpoints::things_create::<S, N, Si, Cmd>))
                .route(web::put().to(endpoints::things_update::<S, N, Si, Cmd>)),
        );
        ctx.service(
            web::resource("/api/v1alpha1/things/{application}/things/{thing}")
                .route(web::get().to(endpoints::things_get::<S, N, Si, Cmd>))
                .route(web::delete().to(endpoints::things_delete::<S, N, Si, Cmd>))
                .route(
                    web::patch()
                        .guard(guard::Header("content-type", "application/json-patch+json"))
                        .to(endpoints::things_patch::<S, N, Si, Cmd>),
                )
                .route(
                    web::patch()
                        .guard(guard::Header(
                            "content-type",
                            "application/merge-patch+json",
                        ))
                        .to(endpoints::things_merge::<S, N, Si, Cmd>),
                ),
        );
        ctx.service(
            web::resource("/api/v1alpha1/things/{application}/things/{thing}/reportedStates")
                .route(web::put().to(endpoints::things_update_reported_state::<S, N, Si, Cmd>)),
        );
        ctx.service(
            web::resource(
                "/api/v1alpha1/things/{application}/things/{thing}/syntheticStates/{name}",
            )
            .route(web::put().to(endpoints::things_update_synthetic_state::<
                S,
                N,
                Si,
                Cmd,
            >)),
        );
        ctx.service(
            web::resource("/api/v1alpha1/things/{application}/things/{thing}/desiredStates/{name}")
                .route(web::put().to(endpoints::things_update_desired_state::<S, N, Si, Cmd>)),
        );
        ctx.service(
            web::resource(
                "/api/v1alpha1/things/{application}/things/{thing}/desiredStates/{name}/value",
            )
            .route(web::put().to(endpoints::things_update_desired_state_value::<S, N, Si, Cmd>)),
        );
        ctx.service(
            web::resource("/api/v1alpha1/things/{application}/things/{thing}/reconciliations")
                .route(web::put().to(endpoints::things_update_reconciliation::<S, N, Si, Cmd>)),
        );
        ctx.service(
            web::resource("/api/v1alpha1/things/{application}/notifications")
                .route(web::get().to(endpoints::things_notifications::<S, N, Si, Cmd>)),
        );
        ctx.service(
            web::resource("/api/v1alpha1/things/{application}/things/{thing}/notifications")
                .route(web::get().to(endpoints::things_notifications_single::<S, N, Si, Cmd>)),
        );
    })
}

pub async fn run(
    config: Config<postgres::Storage, kafka::Notifier, sink::kafka::Sink, mqtt::CommandSink>,
) -> anyhow::Result<()> {
    let mut spawner = vec![];
    let configurator = configure::<_, _, _, _>(&mut spawner, config)?;

    let http = HttpServer::new(move || App::new().configure(|ctx| configurator(ctx)))
        .bind("[::]:8080")?
        .run()
        .map_err(|err| anyhow!(err))
        .boxed_local();

    spawner.spawn(http);

    run_main(spawner).await
}
