use crate::Config;
use actix_web::web::Json;
use actix_web::{web, HttpResponse};
use drogue_doppelgaenger_common::error::ErrorInformation;
use drogue_doppelgaenger_common::model::Thing;
use drogue_doppelgaenger_common::service::Service;
use serde_json::json;

macro_rules! ensure_app {
    ($config:expr, $application:expr) => {
        if $config.instance != $application {
            return Ok(HttpResponse::NotFound().json(ErrorInformation {
                error: "NotFound".to_string(),
                message: None,
            }));
        }
    };
}

pub async fn things_get<S: Service>(
    config: web::Data<Config<S>>,
    service: web::Data<S>,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, actix_web::Error> {
    let (application, thing) = path.into_inner();
    ensure_app!(config, application);

    let result = service.get(&thing).await?;

    Ok(HttpResponse::Ok().json(result))
}

pub async fn things_create<S: Service>(
    config: web::Data<Config<S>>,
    service: web::Data<S>,
    payload: Json<Thing>,
) -> Result<HttpResponse, actix_web::Error> {
    ensure_app!(config, payload.metadata.application);

    service.create(payload.into_inner()).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_update<S: Service>(
    config: web::Data<Config<S>>,
    service: web::Data<S>,
    payload: Json<Thing>,
) -> Result<HttpResponse, actix_web::Error> {
    ensure_app!(config, payload.metadata.application);

    service.update(payload.into_inner()).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}

pub async fn things_delete<S: Service>(
    config: web::Data<Config<S>>,
    service: web::Data<S>,
    path: web::Path<(String, String)>,
) -> Result<HttpResponse, actix_web::Error> {
    let (application, thing) = path.into_inner();
    ensure_app!(config, application);

    service.delete(&thing).await?;

    Ok(HttpResponse::NoContent().json(json!({})))
}
