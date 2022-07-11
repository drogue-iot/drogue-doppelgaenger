pub mod redis;

use crate::error::ErrorInformation;
use crate::model::Thing;
use actix_web::body::BoxBody;
use actix_web::{HttpResponse, ResponseError};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::fmt::Debug;

#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
    #[error("Not found")]
    NotFound,
    #[error("Already exists")]
    AlreadyExists,
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Internal Error: {0}")]
    Internal(#[source] E),
}

#[async_trait]
pub trait Service: Sized + Send + Sync + 'static {
    type Config: Clone + Send + Sync + 'static;
    type Error: std::error::Error + Debug;

    fn new(config: Self::Config) -> anyhow::Result<Self>;

    async fn get(&self, name: &str) -> Result<Thing, Error<Self::Error>>;
    async fn create(&self, thing: Thing) -> Result<(), Error<Self::Error>>;
    async fn update(&self, thing: Thing) -> Result<(), Error<Self::Error>>;
    async fn delete(&self, name: &str) -> Result<(), Error<Self::Error>>;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Internal {
    pub wakeup: Option<DateTime<Utc>>,
}

impl<E: std::error::Error + Debug> ResponseError for Error<E> {
    fn error_response(&self) -> HttpResponse<BoxBody> {
        match self {
            Error::NotFound => HttpResponse::NotFound().finish(),
            Error::AlreadyExists => HttpResponse::Conflict().json(ErrorInformation {
                error: "AlreadyExists".to_string(),
                message: Some(self.to_string()),
            }),
            Error::Serialization(err) => err.error_response(),
            Error::Internal(err) => HttpResponse::InternalServerError().json(ErrorInformation {
                error: "InternalError".to_string(),
                message: Some(err.to_string()),
            }),
        }
    }
}
