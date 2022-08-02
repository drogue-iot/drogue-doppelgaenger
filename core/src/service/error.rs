use crate::{
    command::CommandSink,
    error::ErrorInformation,
    machine, notifier,
    notifier::Notifier,
    storage::{self, Storage},
};
use actix_web::{body::BoxBody, HttpResponse, ResponseError};
use std::fmt::{Debug, Formatter};

#[derive(thiserror::Error)]
pub enum Error<S: Storage, N: Notifier, Cmd: CommandSink> {
    #[error("Storage: {0}")]
    Storage(#[source] storage::Error<S::Error>),
    #[error("Notifier: {0}")]
    Notifier(#[source] notifier::Error<N::Error>),
    #[error("State Machine: {0}")]
    Machine(#[from] machine::Error),
    #[error("Command sink: {0}")]
    Command(#[source] Cmd::Error),
    #[error("Unclean Outbox")]
    UncleanOutbox,
}

impl<S: Storage, N: Notifier, Cmd: CommandSink> Debug for Error<S, N, Cmd> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(err) => f.debug_tuple("Storage").field(err).finish(),
            Self::Notifier(err) => f.debug_tuple("Notifier").field(err).finish(),
            Self::Machine(err) => f.debug_tuple("Machine").field(err).finish(),
            Self::Command(err) => f.debug_tuple("Command").field(err).finish(),
            Self::UncleanOutbox => f.debug_tuple("UncleanOutbox").finish(),
        }
    }
}

impl<S, N, Cmd> ResponseError for Error<S, N, Cmd>
where
    S: Storage,
    N: Notifier,
    Cmd: CommandSink,
{
    fn error_response(&self) -> HttpResponse<BoxBody> {
        match self {
            Error::Storage(storage::Error::NotFound) => HttpResponse::NotFound().finish(),
            Error::Storage(storage::Error::AlreadyExists) => {
                HttpResponse::Conflict().json(ErrorInformation {
                    error: "AlreadyExists".to_string(),
                    message: Some(self.to_string()),
                })
            }
            Error::Storage(storage::Error::PreconditionFailed) => {
                HttpResponse::PreconditionFailed().finish()
            }
            Error::Storage(storage::Error::Serialization(err)) => err.error_response(),

            err => HttpResponse::InternalServerError().json(ErrorInformation {
                error: "InternalError".to_string(),
                message: Some(err.to_string()),
            }),
        }
    }
}
