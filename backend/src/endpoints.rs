use crate::{
    notifier::actix::WebSocketHandler,
    utils::{to_datetime, to_duration},
    Instance,
};
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use chrono::Utc;
use drogue_bazaar::auth::UserInformation;
use drogue_doppelgaenger_core::{
    command::CommandSink,
    listener::KafkaSource,
    model::{Reconciliation, SyntheticType, Thing},
    notifier::Notifier,
    processor::{sink::Sink, SetDesiredValue},
    service::{
        AnnotationsUpdater, DesiredStateUpdate, DesiredStateUpdater, DesiredStateValueUpdater, Id,
        JsonMergeUpdater, JsonPatchUpdater, Patch, ReportedStateUpdater, Service,
        SyntheticStateUpdater, UpdateMode, UpdateOptions,
    },
    storage::Storage,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const OPTS: UpdateOptions = UpdateOptions {
    ignore_unclean_inbox: true,
};

pub async fn things_get<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<Id>,
) -> Result<HttpResponse, actix_web::Error> {
    Ok(match service.get(&path.into_inner()).await? {
        Some(thing) => HttpResponse::Ok().json(thing),
        None => HttpResponse::NotFound().finish(),
    })
}

pub async fn things_create<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    payload: web::Json<Thing>,
) -> Result<HttpResponse, actix_web::Error> {
    service.create(payload.into_inner()).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    payload: web::Json<Thing>,
) -> Result<HttpResponse, actix_web::Error> {
    let application = payload.metadata.application.clone();
    let thing = payload.metadata.name.clone();
    let payload = payload.into_inner();

    service
        .update(&Id { application, thing }, &payload, &OPTS)
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_patch<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<Id>,
    payload: web::Json<Patch>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service
        .update(&path.into_inner(), &JsonPatchUpdater(payload), &OPTS)
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_merge<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<Id>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service
        .update(&path.into_inner(), &JsonMergeUpdater(payload), &OPTS)
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_reported_state<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<Id>,
    payload: web::Json<BTreeMap<String, Value>>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service
        .update(
            &path.into_inner(),
            &ReportedStateUpdater(payload, UpdateMode::Merge),
            &OPTS,
        )
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_synthetic_state<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<(String, String, String)>,
    payload: web::Json<SyntheticType>,
) -> Result<HttpResponse, actix_web::Error> {
    let (application, thing, state) = path.into_inner();
    let payload = payload.into_inner();

    service
        .update(
            &Id::new(application, thing),
            &SyntheticStateUpdater(state, payload),
            &OPTS,
        )
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_desired_state<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<(String, String, String)>,
    payload: web::Json<DesiredStateUpdate>,
) -> Result<HttpResponse, actix_web::Error> {
    let (application, thing, state) = path.into_inner();
    let payload = payload.into_inner();

    service
        .update(
            &Id::new(application, thing),
            &DesiredStateUpdater(state, payload),
            &OPTS,
        )
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_desired_state_value<
    S: Storage,
    N: Notifier,
    Si: Sink,
    Cmd: CommandSink,
>(
    request: HttpRequest,
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<(String, String, String)>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, actix_web::Error> {
    let (application, thing, name) = path.into_inner();
    let value = payload.into_inner();

    let valid_until = request
        .headers()
        .get("valid-until")
        .map(to_datetime)
        .transpose()?;
    let valid_for = request
        .headers()
        .get("valid-for")
        .map(to_duration)
        .transpose()?;

    let valid_until = valid_until.or_else(|| valid_for.map(|d| Utc::now() + d));

    let mut values = BTreeMap::new();
    values.insert(name, SetDesiredValue::WithOptions { value, valid_until });

    service
        .update(
            &Id::new(application, thing),
            &DesiredStateValueUpdater(values),
            &OPTS,
        )
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_reconciliation<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<Id>,
    payload: web::Json<Reconciliation>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service.update(&path.into_inner(), &payload, &OPTS).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_annotations<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<Id>,
    payload: web::Json<BTreeMap<String, Option<String>>>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service
        .update(&path.into_inner(), &AnnotationsUpdater(payload), &OPTS)
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_delete<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    service: web::Data<Service<S, N, Si, Cmd>>,
    path: web::Path<Id>,
) -> Result<HttpResponse, actix_web::Error> {
    // FIXME: allow adding preconditions
    service.delete(&path.into_inner(), None).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_notifications<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    req: HttpRequest,
    path: web::Path<String>,
    stream: web::Payload,
    source: web::Data<KafkaSource>,
    service: web::Data<Service<S, N, Si, Cmd>>,
    instance: web::Data<Instance>,
) -> Result<HttpResponse, actix_web::Error> {
    let application = path.into_inner();
    if let Some(expected_application) = &instance.application {
        if expected_application != &application {
            return Ok(HttpResponse::NotFound().finish());
        }
    }

    let handler =
        WebSocketHandler::new(service.into_inner(), source.into_inner(), application, None);
    ws::start(handler, &req, stream)
}

pub async fn things_notifications_single<S: Storage, N: Notifier, Si: Sink, Cmd: CommandSink>(
    req: HttpRequest,
    path: web::Path<(String, String)>,
    stream: web::Payload,
    source: web::Data<KafkaSource>,
    service: web::Data<Service<S, N, Si, Cmd>>,
    instance: web::Data<Instance>,
    user: UserInformation,
) -> Result<HttpResponse, actix_web::Error> {
    log::info!("Start single notification: {user:?}");

    let (application, thing) = path.into_inner();
    if let Some(expected_application) = &instance.application {
        if expected_application != &application {
            return Ok(HttpResponse::NotFound().finish());
        }
    }

    let handler = WebSocketHandler::new(
        service.into_inner(),
        source.into_inner(),
        application,
        Some(thing),
    );
    ws::start(handler, &req, stream)
}
