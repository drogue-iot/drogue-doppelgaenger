use crate::notifier::actix::WebSocketHandler;
use crate::Instance;
use actix_web::{web, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use drogue_doppelgaenger_core::processor::sink::Sink;
use drogue_doppelgaenger_core::{
    listener::KafkaSource,
    model::{Reconciliation, Thing},
    notifier::Notifier,
    service::{
        Id, JsonMergeUpdater, JsonPatchUpdater, Patch, ReportedStateUpdater, Service, UpdateMode,
    },
    storage::Storage,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub async fn things_get<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    path: web::Path<Id>,
) -> Result<HttpResponse, actix_web::Error> {
    let result = service.get(&path.into_inner()).await?;

    Ok(HttpResponse::Ok().json(result))
}

pub async fn things_create<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    payload: web::Json<Thing>,
) -> Result<HttpResponse, actix_web::Error> {
    service.create(payload.into_inner()).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    payload: web::Json<Thing>,
) -> Result<HttpResponse, actix_web::Error> {
    let application = payload.metadata.application.clone();
    let thing = payload.metadata.name.clone();
    let payload = payload.into_inner();

    service.update(&Id { application, thing }, payload).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_patch<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    path: web::Path<Id>,
    payload: web::Json<Patch>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service
        .update(&path.into_inner(), JsonPatchUpdater(payload))
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_merge<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    path: web::Path<Id>,
    payload: web::Json<Value>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service
        .update(&path.into_inner(), JsonMergeUpdater(payload))
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_reported_state<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    path: web::Path<Id>,
    payload: web::Json<BTreeMap<String, Value>>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service
        .update(
            &path.into_inner(),
            ReportedStateUpdater(payload, UpdateMode::Merge),
        )
        .await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update_reconciliation<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    path: web::Path<Id>,
    payload: web::Json<Reconciliation>,
) -> Result<HttpResponse, actix_web::Error> {
    let payload = payload.into_inner();

    service.update(&path.into_inner(), payload).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_delete<S: Storage, N: Notifier, Si: Sink>(
    service: web::Data<Service<S, N, Si>>,
    path: web::Path<Id>,
) -> Result<HttpResponse, actix_web::Error> {
    service.delete(&path.into_inner()).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_notifications<S: Storage, N: Notifier, Si: Sink>(
    req: HttpRequest,
    path: web::Path<String>,
    stream: web::Payload,
    source: web::Data<KafkaSource>,
    service: web::Data<Service<S, N, Si>>,
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

pub async fn things_notifications_single<S: Storage, N: Notifier, Si: Sink>(
    req: HttpRequest,
    path: web::Path<(String, String)>,
    stream: web::Payload,
    source: web::Data<KafkaSource>,
    service: web::Data<Service<S, N, Si>>,
    instance: web::Data<Instance>,
) -> Result<HttpResponse, actix_web::Error> {
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
