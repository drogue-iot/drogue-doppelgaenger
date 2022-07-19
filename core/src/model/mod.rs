use base64::STANDARD;
use base64_serde::base64_serde_type;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::BTreeMap;

/// The full thing model.
#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
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
        }
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
    Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Reconciliation {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub changed: BTreeMap<String, Changed>,
}

impl Reconciliation {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Changed {
    Script(String),
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
pub struct DesiredFeature {
    pub last_update: DateTime<Utc>,
    pub value: Value,
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticFeature {
    pub script: Script,
    pub last_update: DateTime<Utc>,
    pub value: Value,
}

base64_serde_type!(Base64Standard, STANDARD);

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Script {
    #[serde(rename = "javaScript")]
    JavaScript(String),
    #[serde(rename = "wasm")]
    WASM(
        #[serde(with = "Base64Standard")]
        #[schemars(with = "String")]
        Vec<u8>,
    ),
}

#[cfg(test)]
mod test {
    use super::*;
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
}
