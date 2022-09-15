mod desired;
mod recon;
pub mod types;

pub use desired::*;
pub use recon::*;

use base64::STANDARD;
use base64_serde::base64_serde_type;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

pub trait InternalState: Sized {
    fn is_empty(&self) -> bool;

    fn is_none_or_empty(internal: &Option<Self>) -> bool {
        internal.as_ref().map(|i| i.is_empty()).unwrap_or(true)
    }
}

impl InternalState for Value {
    fn is_empty(&self) -> bool {
        self.is_null()
    }
}

/// The full thing model.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Thing<I = Value>
where
    I: InternalState,
{
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

    #[serde(
        default = "default_internal",
        skip_serializing_if = "InternalState::is_none_or_empty"
    )]
    #[serde(bound(deserialize = "Option<I>: serde::Deserialize<'de>"))]
    #[schemars(skip)]
    pub internal: Option<I>,
}

const fn default_internal<I>() -> Option<I> {
    None
}

impl<I> Thing<I>
where
    I: InternalState,
{
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

    /// Replace the internal section with `None`.
    pub fn strip_internal<MI>(self) -> Thing<MI>
    where
        MI: InternalState,
    {
        self.map_internal(|_| None)
    }

    /// Map the internal section into another
    pub fn map_internal<F, MI>(self, f: F) -> Thing<MI>
    where
        MI: InternalState,
        F: FnOnce(I) -> Option<MI>,
    {
        let Thing {
            metadata,
            schema,
            reported_state,
            desired_state,
            synthetic_state,
            reconciliation,
            internal,
        } = self;
        let internal = internal.and_then(f);
        Thing {
            metadata,
            schema,
            reported_state,
            desired_state,
            synthetic_state,
            reconciliation,
            internal,
        }
    }
}

impl<I> Thing<I>
where
    I: InternalState + Serialize,
{
    pub fn into_external(self) -> Thing<Value> {
        self.map_internal(|i| serde_json::to_value(i).ok())
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

impl<I: InternalState> From<Thing<I>> for ThingState {
    fn from(thing: Thing<I>) -> Self {
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

impl<I: InternalState> From<&Thing<I>> for ThingState {
    fn from(thing: &Thing<I>) -> Self {
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

/// Evaluate if the value is equal to its types default value.
///
/// This is intended to be use with serde's `skip_serializing_if`. But keep in mind, that this will
/// create a new (default) instant of the type for every check.
pub fn is_default<T>(value: &T) -> bool
where
    T: Default + PartialEq,
{
    value == &T::default()
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn test_ser_schema() {
        let mut thing: Thing = Thing::new("app", "thing");
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
        let mut thing: Thing = Thing::new("app", "thing");
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

    #[test]
    fn test_internal() {
        #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
        struct MyInternal {
            pub foo: String,
            pub bar: String,
        }

        impl InternalState for MyInternal {
            fn is_empty(&self) -> bool {
                self.foo.is_empty() && self.bar.is_empty()
            }
        }

        let json = json!({
            "metadata": {
                "name": "thing",
                "application": "app",
            },
            "internal": {
                "foo": "f1",
                "bar": "b1",
            }
        });

        // unstructured data should deserialize
        let t: Thing<Value> = serde_json::from_value(json).unwrap();
        assert_eq!(
            t.internal,
            Some(json!({
                "foo": "f1",
                "bar": "b1",
            }))
        );

        // an re-serialize the same way
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(
            json,
            json!({
                "metadata": {
                    "name": "thing",
                    "application": "app",
                },
                "internal": {
                    "foo": "f1",
                    "bar": "b1",
                }
            })
        );

        // and deserialize into the actual internal structure
        let mut t: Thing<MyInternal> = serde_json::from_value(json).unwrap();
        assert_eq!(
            t.internal,
            Some(MyInternal {
                foo: "f1".to_string(),
                bar: "b1".to_string()
            })
        );

        // make it "empty"
        if let Some(internal) = &mut t.internal {
            internal.foo = "".to_string();
            internal.bar = "".to_string();
        }

        // serialize and check
        let json = serde_json::to_value(&t).unwrap();
        assert_eq!(
            json,
            json! ({
                "metadata": {
                    "name": "thing",
                    "application": "app",
                }
            })
        );
    }
}
