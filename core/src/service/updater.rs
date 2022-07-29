use crate::model::{
    DesiredFeature, DesiredFeatureMethod, DesiredFeatureReconciliation, DesiredMode,
    Reconciliation, ReportedFeature, SyntheticFeature, SyntheticType, Thing,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::{btree_map::Entry, BTreeMap};
use std::convert::Infallible;
use std::time::Duration;

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

impl UpdateMode {
    pub fn from_partial(partial: bool) -> Self {
        match partial {
            true => Self::Merge,
            false => Self::Replace,
        }
    }
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
                                // don't need to update the last_updated, as the system will ensure
                                // that for us later on
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

pub struct SyntheticStateUpdater(pub String, pub SyntheticType);

impl InfallibleUpdater for SyntheticStateUpdater {
    fn update(self, mut thing: Thing) -> Thing {
        match thing.synthetic_state.entry(self.0) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().r#type = self.1;
            }
            Entry::Vacant(entry) => {
                entry.insert(SyntheticFeature {
                    r#type: self.1,
                    last_update: Utc::now(),
                    value: Default::default(),
                });
            }
        }

        thing
    }
}

/// A more flexible update struct for [`DesiredFeature`].
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesiredStateUpdate {
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "humantime_serde")]
    pub valid_for: Option<Duration>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DesiredMode>,

    #[serde(default)]
    pub reconciliation: Option<DesiredFeatureReconciliation>,
    #[serde(default)]
    pub method: Option<DesiredFeatureMethod>,
}

#[derive(Debug, thiserror::Error)]
pub enum DesiredStateUpdaterError {
    #[error("Out of range: {0}")]
    OutOfRange(#[from] time::OutOfRangeError),
}

pub struct DesiredStateUpdater(pub String, pub DesiredStateUpdate);

impl Updater for DesiredStateUpdater {
    type Error = DesiredStateUpdaterError;

    fn update(self, mut thing: Thing) -> Result<Thing, DesiredStateUpdaterError> {
        let DesiredStateUpdate {
            value,
            valid_until,
            valid_for,
            reconciliation,
            method,
            mode,
        } = self.1;

        let valid_until = valid_until.or(valid_for
            .map(|d| chrono::Duration::from_std(d))
            .transpose()?
            .map(|d| Utc::now() + d));

        match thing.desired_state.entry(self.0) {
            Entry::Occupied(mut entry) => {
                // we update what we got
                let entry = entry.get_mut();
                if let Some(value) = value {
                    entry.value = value;
                }
                entry.valid_until = valid_until;
                if let Some(reconciliation) = reconciliation {
                    entry.reconciliation = reconciliation;
                }
                if let Some(method) = method {
                    entry.method = method;
                }
                if let Some(mode) = mode {
                    entry.mode = mode;
                }
            }
            Entry::Vacant(entry) => {
                // we create some reasonable defaults
                entry.insert(DesiredFeature {
                    value: value.unwrap_or_default(),
                    last_update: Utc::now(),
                    valid_until,
                    reconciliation: reconciliation.unwrap_or_default(),
                    method: method.unwrap_or_default(),
                    mode: mode.unwrap_or_default(),
                });
            }
        }

        Ok(thing)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DesiredStateValueUpdaterError {
    #[error("Unknown feature: {0}")]
    Unknown(String),
}

pub struct DesiredStateValueUpdater {
    pub name: String,
    pub value: Value,
    pub valid_until: Option<DateTime<Utc>>,
}

impl Updater for DesiredStateValueUpdater {
    type Error = DesiredStateValueUpdaterError;

    fn update(self, mut thing: Thing) -> Result<Thing, Self::Error> {
        let state = thing
            .desired_state
            .get_mut(&self.name)
            .ok_or_else(|| DesiredStateValueUpdaterError::Unknown(self.name))?;
        state.value = self.value;
        state.valid_until = self.valid_until;
        Ok(thing)
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
