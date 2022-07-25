use crate::model::{Reconciliation, ReportedFeature, Thing};
use chrono::Utc;
use serde_json::Value;
use std::collections::{btree_map::Entry, BTreeMap};
use std::convert::Infallible;

pub use json_patch::Patch;

pub trait Updater {
    type Error: std::error::Error + 'static;

    fn update(self, thing: Thing) -> Result<Thing, Self::Error>;
}

pub trait InfallibleUpdater {
    fn update(self, thing: Thing) -> Thing;
}

impl<I> Updater for I
where
    I: InfallibleUpdater,
{
    type Error = Infallible;

    fn update(self, thing: Thing) -> Result<Thing, Self::Error> {
        Ok(InfallibleUpdater::update(self, thing))
    }
}

pub enum UpdateMode {
    Merge,
    Replace,
}

pub struct ReportedStateUpdater(pub BTreeMap<String, Value>, pub UpdateMode);

impl InfallibleUpdater for ReportedStateUpdater {
    fn update(self, mut thing: Thing) -> Thing {
        match self.1 {
            // merge new data into current, update timestamps when the value has indeed changed
            UpdateMode::Merge => {
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
                            e.insert(ReportedFeature::now(value));
                        }
                    }
                }
            }
            // the new data set is the new state, but update timestamps only when the old
            // data differed from the newly provided.
            UpdateMode::Replace => {
                let mut new_state = BTreeMap::new();
                for (key, value) in self.0 {
                    match thing.reported_state.remove_entry(&key) {
                        Some((key, feature)) => {
                            if feature.value == value {
                                new_state.insert(key, feature);
                            } else {
                                new_state.insert(key, ReportedFeature::now(value));
                            }
                        }
                        None => {
                            new_state.insert(key, ReportedFeature::now(value));
                        }
                    }
                }
                thing.reported_state = new_state;
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

/// Updater for JSON patch
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

/// Updater for JSON merge
pub struct JsonMergeUpdater(pub Value);

#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct MergeError(#[from] serde_json::Error);

impl Updater for JsonMergeUpdater {
    type Error = MergeError;

    fn update(self, thing: Thing) -> Result<Thing, Self::Error> {
        let mut json = serde_json::to_value(thing)?;
        json_patch::merge(&mut json, &self.0);
        Ok(serde_json::from_value(json)?)
    }
}

#[cfg(test)]
mod test {

    use super::InfallibleUpdater;
    use super::*;
    use serde_json::Value;

    fn new_thing() -> Thing {
        Thing::new("default", "test")
    }

    #[test]
    fn test_repstate_merge_empty() {
        let thing = new_thing();

        let mut data = BTreeMap::<String, Value>::new();
        data.insert("foo".into(), "bar".into());
        let mut thing =
            InfallibleUpdater::update(ReportedStateUpdater(data, UpdateMode::Merge), thing);

        assert_eq!(
            thing.reported_state.remove("foo").map(|s| s.value),
            Some(Value::String("bar".to_string()))
        );
    }

    #[test]
    fn test_repstate_replace_empty() {
        let thing = new_thing();

        let mut data = BTreeMap::<String, Value>::new();
        data.insert("foo".into(), "bar".into());
        let mut thing =
            InfallibleUpdater::update(ReportedStateUpdater(data, UpdateMode::Replace), thing);

        assert_eq!(
            thing.reported_state.remove("foo").map(|s| s.value),
            Some(Value::String("bar".to_string()))
        );
    }
}
