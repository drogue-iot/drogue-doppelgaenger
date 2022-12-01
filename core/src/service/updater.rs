use crate::{
    model::{
        Deleting, DesiredFeature, DesiredFeatureMethod, DesiredFeatureReconciliation, DesiredMode,
        Reconciliation, ReportedFeature, SyntheticFeature, SyntheticType, Thing,
    },
    processor::SetDesiredValue,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::{
    collections::{btree_map::Entry, BTreeMap},
    convert::Infallible,
    fmt::Debug,
    time::Duration,
};

use crate::model::Internal;
pub use json_patch::Patch;

pub trait Updater {
    type Error: std::error::Error + Send + Sync + 'static;

    fn update(&self, thing: Thing<Internal>) -> Result<Thing<Internal>, Self::Error>;
}

pub trait UpdaterExt: Updater + Sized {
    fn and_then<U>(self, updater: U) -> AndThenUpdater<Self, U>
    where
        U: Updater,
    {
        AndThenUpdater(self, updater)
    }
}

impl<T> UpdaterExt for T where T: Updater {}

#[derive(Debug, thiserror::Error)]
pub enum AndThenError<E1, E2>
where
    E1: std::error::Error,
    E2: std::error::Error,
{
    #[error("First: {0}")]
    First(#[source] E1),
    #[error("Second: {0}")]
    Second(#[source] E2),
}

pub struct AndThenUpdater<U1, U2>(U1, U2)
where
    U1: Updater,
    U2: Updater;

impl<U1, U2> Updater for AndThenUpdater<U1, U2>
where
    U1: Updater,
    U2: Updater,
{
    type Error = AndThenError<U1::Error, U2::Error>;

    fn update(&self, thing: Thing<Internal>) -> Result<Thing<Internal>, Self::Error> {
        Ok(self
            .1
            .update(self.0.update(thing).map_err(AndThenError::First)?)
            .map_err(AndThenError::Second)?)
    }
}

pub trait InfallibleUpdater {
    fn update(&self, thing: Thing<Internal>) -> Thing<Internal>;
}

impl<I> Updater for I
where
    I: InfallibleUpdater,
{
    type Error = Infallible;

    fn update(&self, thing: Thing<Internal>) -> Result<Thing<Internal>, Self::Error> {
        Ok(InfallibleUpdater::update(self, thing))
    }
}

impl InfallibleUpdater for () {
    fn update(&self, thing: Thing<Internal>) -> Thing<Internal> {
        thing
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

pub struct MapValueInserter(pub String, pub String);

impl InfallibleUpdater for MapValueInserter {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        match thing.reported_state.entry(self.0.clone()) {
            Entry::Occupied(mut entry) => {
                let e = entry.get_mut();
                match &mut e.value {
                    Value::Object(fields) => {
                        fields.insert(self.1.clone(), Value::Null);
                    }
                    _ => {
                        *e = ReportedFeature::now(json!({ self.1.clone(): null }));
                    }
                }
            }
            Entry::Vacant(entry) => {
                entry.insert(ReportedFeature::now(json!({ self.1.clone(): null })));
            }
        }

        thing
    }
}

/// Cleanup, in case a reported value is empty
///
/// NOTE: This only works for calls which respect a change on the `deletion_timestamp` field, which
/// currently only the processor does.
pub struct Cleanup(pub String);

impl InfallibleUpdater for Cleanup {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        if thing
            .reported_state
            .get(&self.0)
            .map(|f| match &f.value {
                Value::Object(map) => map.is_empty(),
                Value::Array(array) => array.is_empty(),
                // all other types are considered "empty"
                _ => true,
            })
            .unwrap_or(true)
        {
            log::debug!(
                "Reference is empty, scheduling deletion of {}/{}",
                thing.metadata.application,
                thing.metadata.name
            );
            // mark deleted
            thing.metadata.deletion_timestamp = Some(Utc::now());
        }

        thing
    }
}

pub enum StateType {
    Reported,
    Synthetic,
    Desired,
}

pub struct MapValueRemover(pub String, pub String);

impl InfallibleUpdater for MapValueRemover {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        match thing.reported_state.entry(self.0.clone()) {
            Entry::Occupied(mut entry) => match &mut entry.get_mut().value {
                Value::Object(fields) => {
                    fields.remove(&self.1);
                }
                _ => {
                    // nothing to do
                }
            },
            Entry::Vacant(_) => {
                // nothing to do
            }
        }

        thing
    }
}

/// process a reported state update
pub struct ReportedStateUpdater(pub BTreeMap<String, Value>, pub UpdateMode);

impl InfallibleUpdater for ReportedStateUpdater {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        match self.1 {
            // merge new data into current, update timestamps when the value has indeed changed
            UpdateMode::Merge => {
                for (key, value) in self.0.clone() {
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
                for (key, value) in self.0.clone() {
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
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        thing.reconciliation = self.clone();
        thing
    }
}

impl InfallibleUpdater for Thing<Internal> {
    fn update(&self, _: Thing<Internal>) -> Thing<Internal> {
        self.clone()
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

    fn update(&self, thing: Thing<Internal>) -> Result<Thing<Internal>, Self::Error> {
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

    fn update(&self, thing: Thing<Internal>) -> Result<Thing<Internal>, Self::Error> {
        let mut json = serde_json::to_value(thing)?;
        json_patch::merge(&mut json, &self.0);
        Ok(serde_json::from_value(json)?)
    }
}

pub struct SyntheticStateUpdater(pub String, pub SyntheticType);

impl InfallibleUpdater for SyntheticStateUpdater {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        match thing.synthetic_state.entry(self.0.clone()) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().r#type = self.1.clone();
            }
            Entry::Vacant(entry) => {
                entry.insert(SyntheticFeature {
                    r#type: self.1.clone(),
                    last_update: Utc::now(),
                    value: Default::default(),
                });
            }
        }

        thing
    }
}

/// A more flexible update struct for [`DesiredFeature`].
#[derive(Clone, Debug, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesiredStateUpdate {
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(with = "humantime_serde")]
    #[schemars(schema_with = "drogue_doppelgaenger_model::types::humantime")]
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

    fn update(
        &self,
        mut thing: Thing<Internal>,
    ) -> Result<Thing<Internal>, DesiredStateUpdaterError> {
        let DesiredStateUpdate {
            value,
            valid_until,
            valid_for,
            reconciliation,
            method,
            mode,
        } = self.1.clone();

        let valid_until = valid_until.or(valid_for
            .map(chrono::Duration::from_std)
            .transpose()?
            .map(|d| Utc::now() + d));

        match thing.desired_state.entry(self.0.clone()) {
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
    #[error("Unknown features: {0:?}")]
    Unknown(Vec<String>),
}

pub struct DesiredStateValueUpdater(pub BTreeMap<String, SetDesiredValue>);

impl Updater for DesiredStateValueUpdater {
    type Error = DesiredStateValueUpdaterError;

    fn update(&self, mut thing: Thing<Internal>) -> Result<Thing<Internal>, Self::Error> {
        let mut missing = vec![];

        for (name, set) in self.0.clone() {
            if let Some(state) = thing.desired_state.get_mut(&name) {
                match set {
                    SetDesiredValue::Value(value) => {
                        state.value = value;
                        state.valid_until = None;
                    }
                    SetDesiredValue::WithOptions { value, valid_until } => {
                        state.value = value;
                        state.valid_until = valid_until;
                    }
                }
            } else {
                missing.push(name.clone());
            }
        }

        if missing.is_empty() {
            Ok(thing)
        } else {
            Err(DesiredStateValueUpdaterError::Unknown(missing))
        }
    }
}

pub struct AnnotationsUpdater(pub BTreeMap<String, Option<String>>);

impl AnnotationsUpdater {
    pub fn new<A: Into<String>, V: Into<String>>(annotation: A, value: V) -> Self {
        let mut map = BTreeMap::new();
        map.insert(annotation.into(), Some(value.into()));
        Self(map)
    }
}

impl InfallibleUpdater for AnnotationsUpdater {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        for (k, v) in &self.0 {
            match v {
                Some(v) => {
                    thing
                        .metadata
                        .annotations
                        .insert(k.to_string(), v.to_string());
                }
                None => {
                    thing.metadata.annotations.remove(k);
                }
            }
        }
        thing
    }
}

impl InfallibleUpdater for IndexMap<String, Deleting> {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        for (k, v) in self {
            thing
                .reconciliation
                .deleting
                .insert(k.to_string(), v.clone());
        }

        thing
    }
}

/// Remove a state property.
///
/// Remove the provided state from the designed state type.
pub struct StateRemover(pub String, pub StateType);

impl InfallibleUpdater for StateRemover {
    fn update(&self, mut thing: Thing<Internal>) -> Thing<Internal> {
        match self.1 {
            StateType::Reported => {
                thing.reported_state.remove(&self.0);
            }
            StateType::Synthetic => {
                thing.synthetic_state.remove(&self.0);
            }
            StateType::Desired => {
                thing.desired_state.remove(&self.0);
            }
        }

        thing
    }
}

#[cfg(test)]
mod test {

    use super::InfallibleUpdater;
    use super::*;
    use serde_json::Value;

    fn new_thing() -> Thing<Internal> {
        Thing::new("default", "test")
    }

    #[test]
    fn test_repstate_merge_empty() {
        let thing = new_thing();

        let mut data = BTreeMap::<String, Value>::new();
        data.insert("foo".into(), "bar".into());
        let mut thing =
            InfallibleUpdater::update(&ReportedStateUpdater(data, UpdateMode::Merge), thing);

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
            InfallibleUpdater::update(&ReportedStateUpdater(data, UpdateMode::Replace), thing);

        assert_eq!(
            thing.reported_state.remove("foo").map(|s| s.value),
            Some(Value::String("bar".to_string()))
        );
    }

    #[test]
    fn test_map_value() {
        let thing = new_thing();

        let thing = InfallibleUpdater::update(
            &MapValueInserter("$children".to_string(), "id1".to_string()),
            thing,
        );
        assert_eq!(
            thing.reported_state["$children"].value,
            json!({
                "id1": null,
            })
        );
        let thing = InfallibleUpdater::update(
            &MapValueInserter("$children".to_string(), "id2".to_string()),
            thing,
        );
        assert_eq!(
            thing.reported_state["$children"].value,
            json!({
                "id1": null,
                "id2": null,
            })
        );
        let thing = InfallibleUpdater::update(
            &MapValueRemover("$children".to_string(), "id1".to_string()),
            thing,
        );
        assert_eq!(
            thing.reported_state["$children"].value,
            json!({
                "id2": null,
            })
        );
        let thing = InfallibleUpdater::update(
            &MapValueRemover("$children".to_string(), "id2".to_string()),
            thing,
        );
        assert_eq!(thing.reported_state["$children"].value, json!({}));
    }
}
