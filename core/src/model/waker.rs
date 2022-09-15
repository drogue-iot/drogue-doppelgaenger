use super::*;
use chrono::{DateTime, Duration, Utc};
use std::collections::BTreeSet;

pub trait WakerExt {
    fn wakeup_at(&mut self, when: DateTime<Utc>, reason: WakerReason);

    fn wakeup(&mut self, delay: Duration, reason: WakerReason) {
        self.wakeup_at(Utc::now() + delay, reason);
    }

    fn clear_wakeup(&mut self, reason: WakerReason);
}

impl WakerExt for Thing<Internal> {
    fn wakeup_at(&mut self, when: DateTime<Utc>, reason: WakerReason) {
        match &mut self.internal {
            Some(internal) => {
                internal.wakeup_at(when, reason);
            }
            None => {
                let mut internal = Internal::default();
                internal.wakeup_at(when, reason);
                self.internal = Some(internal);
            }
        }
    }

    fn clear_wakeup(&mut self, reason: WakerReason) {
        if let Some(internal) = &mut self.internal {
            internal.clear_wakeup(reason);
        }
    }
}

impl WakerExt for Internal {
    fn wakeup_at(&mut self, when: DateTime<Utc>, reason: WakerReason) {
        self.waker.wakeup_at(when, reason);
    }

    fn clear_wakeup(&mut self, reason: WakerReason) {
        self.waker.clear_wakeup(reason);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Waker {
    pub when: Option<DateTime<Utc>>,
    pub why: BTreeSet<WakerReason>,
}

impl Waker {
    pub fn is_empty(&self) -> bool {
        self.when.is_none()
    }
}

impl WakerExt for Waker {
    fn wakeup_at(&mut self, when: DateTime<Utc>, reason: WakerReason) {
        self.why.insert(reason);
        match self.when {
            None => self.when = Some(when),
            Some(w) => {
                if w > when {
                    self.when = Some(when);
                }
            }
        }
    }

    fn clear_wakeup(&mut self, reason: WakerReason) {
        self.why.remove(&reason);
        if self.why.is_empty() {
            self.when = None;
        }
    }
}

#[derive(
    Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum WakerReason {
    Reconcile,
    Outbox,
    Deletion,
}
