use actix_web::{web, HttpRequest, HttpResponse, Responder};
use anyhow::Context;
use serde_json::{json, Value};
use url::Url;

const SPEC: &str = include_str!("../api/openapi.yaml");

#[derive(Clone, Debug)]
pub struct OpenApiConfig {
    pub authorization_url: Option<Url>,
}

pub async fn api(req: HttpRequest, openapi: web::Data<OpenApiConfig>) -> impl Responder {
    match spec(req, openapi.get_ref()) {
        Ok(spec) => HttpResponse::Ok().json(spec),
        Err(err) => {
            log::warn!("Failed to generate OpenAPI spec: {}", err);
            HttpResponse::InternalServerError().finish()
        }
    }
}

fn spec(req: HttpRequest, openapi: &OpenApiConfig) -> anyhow::Result<Value> {
    // load API spec

    let mut api: Value = serde_yaml::from_str(SPEC).context("Failed to parse OpenAPI YAML")?;

    // server

    let ci = req.connection_info();
    let url = format!("{}://{}", ci.scheme(), ci.host());
    api["servers"] = json!([{ "url": url, "description": "Drogue Doppelgänger API" }]);

    // SSO

    if let Some(url) = &openapi.authorization_url {
        api["security"] = json!([{"Drogue Cloud SSO": []}]);
        api["components"]["securitySchemes"] = json!({
            "Drogue Cloud SSO": {
                "type": "oauth2",
                "description": "SSO",
                "flows": {
                    "implicit": {
                        "authorizationUrl": url.to_string(),
                        "scopes": {
                            "openid": "OpenID Connect",
                        }
                    }
                }
            },
        });
    }

    // render

    Ok(api)
}
