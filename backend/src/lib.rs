mod endpoints;
mod notifier;

use actix_web::{guard, web, App, HttpServer};
use anyhow::anyhow;
use drogue_doppelgaenger_core::listener::KafkaSource;
use drogue_doppelgaenger_core::notifier::{kafka, Notifier};
use drogue_doppelgaenger_core::processor::source::{EventStream, Sink};
use drogue_doppelgaenger_core::service::Service;
use drogue_doppelgaenger_core::storage::postgres;
use drogue_doppelgaenger_core::{app::run_main, processor, service, storage::Storage};
use futures::future::LocalBoxFuture;
use futures::{FutureExt, TryFutureExt};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config<S: Storage, N: Notifier> {
    pub application: Option<String>,
    // serde(bound) required as S isn't serializable: https://github.com/serde-rs/serde/issues/1296
    #[serde(bound = "")]
    pub service: service::Config<S, N>,

    pub listener: kafka::Config,

    // FIXME: fix up sink configuration
    pub sink: processor::source::kafka::Config,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub application: Option<String>,
}

pub fn configure<S: Storage, N: Notifier, Si: Sink>(
    config: Config<S, N>,
) -> anyhow::Result<(
    impl Fn(&mut web::ServiceConfig) + Send + Sync + Clone,
    LocalBoxFuture<'static, anyhow::Result<()>>,
)> {
    let (_, sink) = processor::source::kafka::EventStream::new(config.sink)?;

    let service = Service::new(config.service, sink)?;
    let service = web::Data::new(service);

    let (source, runner) = KafkaSource::new(config.listener)?;
    let source = web::Data::new(source);

    let instance = web::Data::new(Instance {
        application: config.application,
    });

    Ok((
        move |ctx: &mut web::ServiceConfig| {
            ctx.app_data(service.clone());
            ctx.app_data(instance.clone());
            ctx.app_data(source.clone());
            ctx.service(
                web::resource("/api/v1alpha1/things")
                    .route(web::post().to(endpoints::things_create::<S, N, Si>))
                    .route(web::put().to(endpoints::things_update::<S, N, Si>)),
            );
            ctx.service(
                web::resource("/api/v1alpha1/things/{application}/things/{thing}")
                    .route(web::get().to(endpoints::things_get::<S, N, Si>))
                    .route(web::delete().to(endpoints::things_delete::<S, N, Si>))
                    .route(
                        web::patch()
                            .guard(guard::Header("content-type", "application/json-patch+json"))
                            .to(endpoints::things_patch::<S, N, Si>),
                    )
                    .route(
                        web::patch()
                            .guard(guard::Header(
                                "content-type",
                                "application/merge-patch+json",
                            ))
                            .to(endpoints::things_merge::<S, N, Si>),
                    ),
            );
            ctx.service(
                web::resource("/api/v1alpha1/things/{application}/things/{thing}/reportedState")
                    .route(web::put().to(endpoints::things_update_reported_state::<S, N, Si>)),
            );
            ctx.service(
                web::resource("/api/v1alpha1/things/{application}/things/{thing}/reconciliation")
                    .route(web::put().to(endpoints::things_update_reconciliation::<S, N, Si>)),
            );
            ctx.service(
                web::resource("/api/v1alpha1/things/{application}/notifications")
                    .route(web::get().to(endpoints::things_notifications::<S, N, Si>)),
            );
            ctx.service(
                web::resource("/api/v1alpha1/things/{application}/things/{thing}/notifications")
                    .route(web::get().to(endpoints::things_notifications_single::<S, N, Si>)),
            );
        },
        async move { runner.run().await }.boxed_local(),
    ))
}

pub async fn run(config: Config<postgres::Storage, kafka::Notifier>) -> anyhow::Result<()> {
    let (configurator, runner) = configure::<_, _, processor::source::kafka::Sink>(config)?;

    let http = HttpServer::new(move || App::new().configure(|ctx| configurator(ctx)))
        .bind("[::]:8080")?
        .run()
        .map_err(|err| anyhow!(err))
        .boxed_local();

    run_main([http, runner]).await
}
