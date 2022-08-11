mod desired;
mod recon;
mod waker;

pub use desired::*;
pub use recon::*;
pub use waker::*;

use crate::{processor::Event, service::Id};
use base64::STANDARD;
use base64_serde::base64_serde_type;
use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The full thing model.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Thing {
    pub metadata: Metadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reported_state: BTreeMap<String, ReportedFeature>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub desired_state: BTreeMap<String, DesiredFeature>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub synthetic_state: BTreeMap<String, SyntheticFeature>,

    #[serde(default, skip_serializing_if = "Reconciliation::is_empty")]
    pub reconciliation: Reconciliation,

    #[serde(default, skip_serializing_if = "Internal::is_none_or_empty")]
    #[schemars(skip)]
    pub internal: Option<Internal>,
}

impl Thing {
    /// Create a new, default, Thing using the required values.
    pub fn new<A: Into<String>, N: Into<String>>(application: A, name: N) -> Self {
        Self {
            metadata: Metadata {
                name: name.into(),
                application: application.into(),
                uid: None,
                creation_timestamp: None,
                deletion_timestamp: None,
                generation: None,
                resource_version: None,
                annotations: Default::default(),
                labels: Default::default(),
            },
            schema: None,
            reported_state: Default::default(),
            desired_state: Default::default(),
            synthetic_state: Default::default(),
            reconciliation: Default::default(),
            internal: None,
        }
    }

    pub fn with_id(id: &Id) -> Self {
        Self::new(&id.application, &id.thing)
    }

    /// Get a clone of the current waker state
    pub fn waker(&self) -> Waker {
        self.internal
            .as_ref()
            .map(|i| i.waker.clone())
            .unwrap_or_default()
    }

    /// Set the waker state, creating an internal if necessary
    pub fn set_waker(&mut self, waker: Waker) {
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

    pub fn outbox(&self) -> &[Event] {
        const EMPTY: [Event; 0] = [];
        self.internal
            .as_ref()
            .map(|internal| internal.outbox.as_slice())
            .unwrap_or(&EMPTY)
    }
}

/// The state view on thing model.
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct ThingState {
    pub metadata: Metadata,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reported_state: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub desired_state: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub synthetic_state: BTreeMap<String, Value>,
}

impl From<Thing> for ThingState {
    fn from(thing: Thing) -> Self {
        Self {
            metadata: thing.metadata,
            reported_state: thing
                .reported_state
                .into_iter()
                .map(|(k, v)| (k, v.value))
                .collect(),
            desired_state: thing
                .desired_state
                .into_iter()
                .map(|(k, v)| (k, v.value))
                .collect(),
            synthetic_state: thing
                .synthetic_state
                .into_iter()
                .map(|(k, v)| (k, v.value))
                .collect(),
        }
    }
}

impl From<&Thing> for ThingState {
    fn from(thing: &Thing) -> Self {
        Self {
            metadata: thing.metadata.clone(),
            reported_state: thing
                .reported_state
                .iter()
                .map(|(k, v)| (k.clone(), v.value.clone()))
                .collect(),
            desired_state: thing
                .desired_state
                .iter()
                .map(|(k, v)| (k.clone(), v.value.clone()))
                .collect(),
            synthetic_state: thing
                .synthetic_state
                .iter()
                .map(|(k, v)| (k.clone(), v.value.clone()))
                .collect(),
        }
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub name: String,
    pub application: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_timestamp: Option<DateTime<Utc>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_version: Option<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Schema {
    Json(JsonSchema),
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "version", content = "schema")]
pub enum JsonSchema {
    Draft7(Value),
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct ReportedFeature {
    pub last_update: DateTime<Utc>,
    pub value: Value,
}

impl ReportedFeature {
    /// Create a new reported feature with the provided value and "now" as timestamp.
    pub fn now(value: Value) -> Self {
        Self {
            value,
            last_update: Utc::now(),
        }
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticFeature {
    #[serde(flatten)]
    pub r#type: SyntheticType,
    pub last_update: DateTime<Utc>,
    pub value: Value,
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum SyntheticType {
    JavaScript(String),
    Alias(String),
}

base64_serde_type!(Base64Standard, STANDARD);

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Code {
    JavaScript(String),
    // FIXME: implement wasm
    /*
    #[serde(rename = "wasm")]
    WASM(
        #[serde(with = "Base64Standard")]
        #[schemars(with = "String")]
        Vec<u8>,
    ),*/
}

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

#[cfg(test)]
mod test {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn test_ser_schema() {
        let mut thing = Thing::new("app", "thing");
        thing.schema = Some(Schema::Json(JsonSchema::Draft7(json!({
            "type": "object",
        }))));
        assert_eq!(
            json!({
                "metadata": {
                    "application": "app",
                    "name": "thing",
                },
                "schema": {
                    "json": {
                        "version": "draft7",
                        "schema": {
                            "type": "object",
                        }
                    }
                }
            }),
            serde_json::to_value(thing).unwrap()
        );
    }

    #[test]
    fn test_ser_syn() {
        let mut thing = Thing::new("app", "thing");
        thing.synthetic_state.insert(
            "foo".to_string(),
            SyntheticFeature {
                r#type: SyntheticType::JavaScript("script".to_string()),
                last_update: Utc.ymd(2022, 1, 1).and_hms(1, 0, 0),
                value: Default::default(),
            },
        );
        assert_eq!(
            json!({
                "metadata": {
                    "name": "thing",
                    "application": "app",
                },
                "syntheticState": {
                    "foo": {
                        "javaScript": "script",
                        "lastUpdate": "2022-01-01T01:00:00Z",
                        "value": null,
                    }
                }
            }),
            serde_json::to_value(thing).unwrap()
        );
    }
}
