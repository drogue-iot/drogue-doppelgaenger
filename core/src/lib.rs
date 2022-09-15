pub mod command;
pub mod config;
pub mod error;
pub mod injector;
pub mod kafka;
pub mod listener;
pub mod machine;
pub mod model;
mod mqtt;
pub mod notifier;
pub mod processor;
pub mod service;
pub mod storage;
pub mod waker;

pub use drogue_bazaar::core::default::is_default;
use drogue_doppelgaenger_model::InternalState;

drogue_bazaar::project!(PROJECT: "Drogue IoT Doppelgänger");

use crate::model::Thing;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preconditions<'o> {
    /// Required resource version.
    pub resource_version: Option<&'o str>,
    /// Required resource UID.
    pub uid: Option<&'o str>,
}

impl<'o, I: InternalState> From<&'o Thing<I>> for Preconditions<'o> {
    fn from(thing: &'o Thing<I>) -> Self {
        Self {
            resource_version: thing.metadata.resource_version.as_deref(),
            uid: thing.metadata.uid.as_deref(),
        }
    }
}

impl Preconditions<'_> {
    pub fn matches<I: InternalState>(&self, thing: &Thing<I>) -> bool {
        if let Some(resource_version) = self.resource_version {
            if Some(resource_version) != thing.metadata.resource_version.as_deref() {
                return false;
            }
        }

        if let Some(uid) = self.uid {
            if Some(uid) != thing.metadata.uid.as_deref() {
                return false;
            }
        }

        return true;
    }
}
