use actix_web::body::BoxBody;
use actix_web::http::header::{HeaderValue, ToStrError};
use actix_web::{HttpResponse, ResponseError};
use chrono::{DateTime, Duration, ParseError, Utc};
use drogue_doppelgaenger_core::error::ErrorInformation;
use humantime::DurationError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Header Value: {0}")]
    Value(#[from] ToStrError),
    #[error("Parse: {0}")]
    Parse(#[from] ParseError),
    #[error("Out of range: {0}")]
    OutOfRange(#[from] time::OutOfRangeError),
    #[error("Duration: {0}")]
    Duration(#[from] DurationError),
}

impl ResponseError for Error {
    fn error_response(&self) -> HttpResponse<BoxBody> {
        HttpResponse::BadRequest().json(ErrorInformation {
            error: "InvalidFormat".to_string(),
            message: Some(self.to_string()),
        })
    }
}

pub fn to_duration(value: &HeaderValue) -> Result<Duration, Error> {
    Ok(Duration::from_std(humantime::parse_duration(
        value.to_str()?,
    )?)?)
}

pub fn to_datetime(value: &HeaderValue) -> Result<DateTime<Utc>, Error> {
    Ok(DateTime::parse_from_rfc3339(value.to_str()?)?.into())
}
