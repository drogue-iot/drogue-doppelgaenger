mod waker;

pub use drogue_doppelgaenger_model::*;
pub use waker::*;

use crate::processor::Event;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Internal {
    #[serde(default, skip_serializing_if = "Waker::is_empty")]
    pub waker: Waker,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outbox: Vec<Event>,
}

impl Internal {
    pub fn is_none_or_empty(internal: &Option<Self>) -> bool {
        internal.as_ref().map(Internal::is_empty).unwrap_or(true)
    }

    pub fn is_empty(&self) -> bool {
        self.waker.is_empty() & self.outbox.is_empty()
    }
}

impl InternalState for Internal {
    fn is_empty(&self) -> bool {
        Internal::is_empty(self)
    }
}

pub trait InternalThingExt {
    /// Get a clone of the current waker state
    fn waker(&self) -> Waker;
    /// Set the waker state, creating an internal if necessary
    fn set_waker(&mut self, waker: Waker);
    fn outbox(&self) -> &[Event];
}

impl InternalThingExt for Thing<Internal> {
    fn waker(&self) -> Waker {
        self.internal
            .as_ref()
            .map(|i| i.waker.clone())
            .unwrap_or_default()
    }

    fn set_waker(&mut self, waker: Waker) {
        if waker.is_empty() {
            // only create an internal if the waker is not empty
            if let Some(internal) = &mut self.internal {
                internal.waker = waker;
            }
        } else {
            match &mut self.internal {
                Some(internal) => internal.waker = waker,
                None => {
                    self.internal = Some(Internal {
                        waker,
                        ..Default::default()
                    })
                }
            }
        }
    }

    fn outbox(&self) -> &[Event] {
        const EMPTY: [Event; 0] = [];
        self.internal
            .as_ref()
            .map(|internal| internal.outbox.as_slice())
            .unwrap_or(&EMPTY)
    }
}
