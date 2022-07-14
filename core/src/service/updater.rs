use crate::model::{Reconciliation, ReportedFeature, Thing};
use crate::service::Updater;
use chrono::Utc;
use serde_json::Value;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;

pub struct ReportedStateUpdater(pub BTreeMap<String, Value>);

impl Updater for ReportedStateUpdater {
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

impl Updater for Reconciliation {
    fn update(self, mut thing: Thing) -> Thing {
        thing.reconciliation = self;
        thing
    }
}

impl Updater for Thing {
    fn update(self, _: Thing) -> Thing {
        self
    }
}
