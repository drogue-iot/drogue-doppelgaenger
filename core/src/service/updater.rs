use crate::model::{Reconciliation, ReportedFeature, Thing};
use crate::service::{InfallibleUpdater, Updater};
use chrono::Utc;
use serde_json::Value;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

pub use json_patch::Patch;

pub struct ReportedStateUpdater(pub BTreeMap<String, Value>);

impl InfallibleUpdater for ReportedStateUpdater {
    fn update(self, mut thing: Thing) -> Thing {
        for (key, value) in self.0 {
            match thing.reported_state.entry(key) {
                Entry::Occupied(mut e) => {
                    let e = e.get_mut();
                    if e.value != value {
                        e.value = value;
                        e.last_update = Utc::now();
                    }
                }
                Entry::Vacant(e) => {
                    e.insert(ReportedFeature {
                        value,
                        last_update: Utc::now(),
                    });
                }
            }
        }
        thing
    }
}

impl InfallibleUpdater for Reconciliation {
    fn update(self, mut thing: Thing) -> Thing {
        thing.reconciliation = self;
        thing
    }
}

impl InfallibleUpdater for Thing {
    fn update(self, _: Thing) -> Thing {
        self
    }
}

pub struct JsonPatchUpdater(pub Patch);

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("Serialization: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Patch: {0}")]
    Patch(#[from] json_patch::PatchError),
}

impl Updater for JsonPatchUpdater {
    type Error = PatchError;

    fn update(self, thing: Thing) -> Result<Thing, Self::Error> {
        let mut json = serde_json::to_value(thing)?;
        json_patch::patch(&mut json, &self.0)?;
        Ok(serde_json::from_value(json)?)
    }
}
