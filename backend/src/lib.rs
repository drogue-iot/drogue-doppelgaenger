mod endpoints;
mod notifier;
mod utils;

use actix_web::web::Json;
use actix_web::{guard, web, Responder};
use drogue_bazaar::{
    actix::{
        auth::{
            authentication::AuthN,
            authorization::{AuthZ, AuthorizerExt, NotAnonymous},
        },
        http::{HttpBuilder, HttpConfig},
    },
    app::Startup,
    auth::{openid, pat},
    client::ClientConfig,
    core::config::ConfigFromEnv,
};
use drogue_client::user;
use drogue_doppelgaenger_core::{
    command::{mqtt, CommandSink},
    listener::KafkaSource,
    notifier::{kafka, Notifier},
    processor::sink::{self, Sink},
    service::{self, Service},
    storage::{postgres, Storage},
    PROJECT,
};
use serde_json::json;

#[derive(Debug, serde::Deserialize)]
pub struct Config<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink> {
    pub application: Option<String>,
    // serde(bound) required as S isn't serializable: https://github.com/serde-rs/serde/issues/1296
    #[serde(bound = "")]
    pub service: service::Config<S, N, Si, Cmd>,

    pub listener: kafka::Config,

    #[serde(default)]
    pub user_auth: Option<ClientConfig>,

    pub oauth: openid::AuthenticatorConfig,
}

#[derive(Clone, Debug)]
pub struct Instance {
    pub application: Option<String>,
}

async fn index() -> impl Responder {
    Json(json!({
        "ok": true,
        "name": PROJECT.name,
        "version": PROJECT.version,
    }))
}

pub async fn configure<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    startup: &mut dyn Startup,
    config: Config<S, N, Si, Cmd>,
) -> anyhow::Result<impl Fn(&mut web::ServiceConfig) + Send + Sync + Clone> {
    let service = Service::from_config(startup, config.service)?;
    let service = web::Data::new(service);

    let source = KafkaSource::new(startup, config.listener)?;
    let source = web::Data::new(source);

    let instance = web::Data::new(Instance {
        application: config.application,
    });

    let authenticator = config.oauth.into_client().await?;
    let user_auth = if let Some(user_auth) = config.user_auth {
        Some(user_auth.into_client::<user::v1::Client>().await?)
    } else {
        None
    };

    if log::log_enabled!(log::Level::Info) {
        log::info!(
            "Authentication: {:?}",
            AuthN::from((
                authenticator.clone(),
                user_auth.clone().map(pat::Authenticator::new),
            ))
        );
    }

    Ok(move |ctx: &mut web::ServiceConfig| {
        let auth = AuthN::from((
            authenticator.clone(),
            user_auth.clone().map(pat::Authenticator::new),
        ));

        ctx.app_data(service.clone());
        ctx.app_data(instance.clone());
        ctx.app_data(source.clone());
        ctx.route("/", web::get().to(index));
        ctx.service(
            web::scope("/api/v1alpha1/things")
                .wrap(AuthZ::new(NotAnonymous.or_else_allow()))
                .wrap(auth)
                .service(
                    web::resource("")
                        .route(web::post().to(endpoints::things_create::<S, N, Si, Cmd>))
                        .route(web::put().to(endpoints::things_update::<S, N, Si, Cmd>)),
                )
                .service(
                    web::resource("/{application}/things/{thing}")
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
                )
                .service(
                    web::resource("/{application}/things/{thing}/reportedStates").route(
                        web::put().to(endpoints::things_update_reported_state::<S, N, Si, Cmd>),
                    ),
                )
                .service(
                    web::resource("/{application}/things/{thing}/syntheticStates/{name}").route(
                        web::put().to(endpoints::things_update_synthetic_state::<S, N, Si, Cmd>),
                    ),
                )
                .service(
                    web::resource("/{application}/things/{thing}/desiredStates/{name}").route(
                        web::put().to(endpoints::things_update_desired_state::<S, N, Si, Cmd>),
                    ),
                )
                .service(
                    web::resource("/{application}/things/{thing}/desiredStates/{name}/value")
                        .route(
                            web::put().to(endpoints::things_update_desired_state_value::<
                                S,
                                N,
                                Si,
                                Cmd,
                            >),
                        ),
                )
                .service(
                    web::resource("/{application}/things/{thing}/reconciliations").route(
                        web::put().to(endpoints::things_update_reconciliation::<S, N, Si, Cmd>),
                    ),
                )
                .service(
                    web::resource("/{application}/things/{thing}/annotations").route(
                        web::put().to(endpoints::things_update_annotations::<S, N, Si, Cmd>),
                    ),
                )
                .service(
                    web::resource("/{application}/notifications")
                        .route(web::get().to(endpoints::things_notifications::<S, N, Si, Cmd>)),
                )
                .service(
                    web::resource("/{application}/things/{thing}/notifications").route(
                        web::get().to(endpoints::things_notifications_single::<S, N, Si, Cmd>),
                    ),
                ),
        );
    })
}

pub async fn run(
    config: Config<postgres::Storage, kafka::Notifier, sink::kafka::Sink, mqtt::CommandSink>,
    startup: &mut dyn Startup,
) -> anyhow::Result<()> {
    let configurator = configure::<_, _, _, _>(startup, config).await?;

    HttpBuilder::new(
        HttpConfig::from_env_prefix("HTTP")?,
        Some(startup.runtime_config()),
        configurator,
    )
    .start(startup)?;

    Ok(())
}
