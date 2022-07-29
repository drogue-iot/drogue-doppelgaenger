use crate::processor::Event;
use crate::service::Id;
use base64::STANDARD;
use base64_serde::base64_serde_type;
use chrono::{DateTime, Duration, Utc};
use indexmap::IndexMap;
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
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub changed: IndexMap<String, Changed>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub timers: IndexMap<String, Timer>,
}

impl Reconciliation {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.timers.is_empty()
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    #[serde(flatten)]
    pub code: Code,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub last_log: Vec<String>,
}

impl From<Code> for Changed {
    fn from(code: Code) -> Self {
        Self {
            code,
            last_log: Default::default(),
        }
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Timer {
    /// the code to run
    #[serde(flatten)]
    pub code: Code,
    /// the period the timer is scheduled
    #[serde(with = "humantime_serde")]
    #[schemars(schema_with = "crate::schemars::humantime")]
    pub period: std::time::Duration,
    /// A flag to stop the timer
    #[serde(default)]
    pub stopped: bool,
    /// the latest timestamp the timer was started
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started: Option<DateTime<Utc>>,
    /// the timestamp the timer last ran
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run: Option<DateTime<Utc>>,
    /// the logs of the last run
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub last_log: Vec<String>,

    /// an optional, initial delay. if there is none, the time will be run the first time it is
    /// configured
    #[serde(with = "humantime_serde")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "crate::schemars::humantime")]
    pub initial_delay: Option<std::time::Duration>,
}

impl Timer {
    pub fn new(
        period: std::time::Duration,
        initial_delay: Option<std::time::Duration>,
        code: Code,
    ) -> Self {
        Self {
            code,
            last_log: Default::default(),
            period: period.into(),
            last_started: None,
            last_run: None,
            stopped: false,
            initial_delay,
        }
    }
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
    /// The value the system desired the device to apply.
    #[serde(default)]
    pub value: Value,
    /// The value the system desired the device to apply.
    #[serde(default)]
    pub mode: DesiredMode,
    /// The timestamp the desired value was last updated.
    pub last_update: DateTime<Utc>,
    /// An optional indication until when the desired value is valid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<DateTime<Utc>>,

    /// The current reconciliation state of the desired value.
    #[serde(default)]
    pub reconciliation: DesiredFeatureReconciliation,
    /// The method of reconciliation.
    #[serde(default)]
    pub method: DesiredFeatureMethod,
}

/// The mode of the desired feature.
#[derive(
    Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum DesiredMode {
    /// Only reconcile once, resulting in success of failure.
    Once,
    /// Keep desired and reported state in sync. Switches back to from "success" to "reconciling"
    /// when the report state deviates from the desired, for as long as the desired value is valid.
    #[default]
    Sync,
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "state")]
pub enum DesiredFeatureReconciliation {
    #[serde(rename_all = "camelCase")]
    Reconciling {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_attempt: Option<DateTime<Utc>>,
    },
    #[serde(rename_all = "camelCase")]
    Succeeded { when: DateTime<Utc> },
    #[serde(rename_all = "camelCase")]
    Failed {
        when: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl Default for DesiredFeatureReconciliation {
    fn default() -> Self {
        Self::Reconciling { last_attempt: None }
    }
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum DesiredFeatureMethod {
    /// Do not process the feature at all.
    ///
    /// NOTE: This will not trigger any state changes other than setting the state to
    /// [`DesiredFeatureState::Reconciling`].
    Manual,
    /// An external process needs to trigger the reconciliation. But the system will detect a
    /// reported state and change the desired state accordingly.
    #[default]
    External,
    /// Try to reconcile the state by sending out commands.
    Command {
        channel: String,
        #[serde(default)]
        payload: DesiredFeatureCommandPayload,
    },
    /// Generate reconcile actions through custom code.
    Code(Code),
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum DesiredFeatureCommandPayload {
    // Send the desired value as payload.
    #[default]
    Raw,
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

pub trait WakerExt {
    fn wakeup_at(&mut self, when: DateTime<Utc>, reason: WakerReason);

    fn wakeup(&mut self, delay: Duration, reason: WakerReason) {
        self.wakeup_at(Utc::now() + delay, reason);
    }

    fn clear_wakeup(&mut self, reason: WakerReason);
}

impl WakerExt for Thing {
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

    #[test]
    fn test_ser_desired() {
        let mut thing = Thing::new("app", "thing");
        thing.desired_state.insert(
            "foo".to_string(),
            DesiredFeature {
                last_update: Utc.ymd(2022, 1, 1).and_hms(1, 2, 3),
                valid_until: Some(Utc.ymd(2022, 1, 1).and_hms(2, 2, 3)),
                value: json!(42),
                reconciliation: DesiredFeatureReconciliation::Reconciling {
                    last_attempt: Some(Utc.ymd(2022, 1, 1).and_hms(1, 2, 4)),
                },
                method: DesiredFeatureMethod::Manual,
                mode: DesiredMode::Sync,
            },
        );
        thing.desired_state.insert(
            "bar".to_string(),
            DesiredFeature {
                last_update: Utc.ymd(2022, 1, 1).and_hms(1, 2, 3),
                valid_until: None,
                value: json!(42),
                reconciliation: DesiredFeatureReconciliation::Succeeded {
                    when: Utc.ymd(2022, 1, 1).and_hms(1, 2, 4),
                },
                method: DesiredFeatureMethod::Manual,
                mode: DesiredMode::Sync,
            },
        );
        thing.desired_state.insert(
            "baz".to_string(),
            DesiredFeature {
                last_update: Utc.ymd(2022, 1, 1).and_hms(1, 2, 3),
                valid_until: None,
                value: json!(42),
                reconciliation: DesiredFeatureReconciliation::Failed {
                    when: Utc.ymd(2022, 1, 1).and_hms(1, 2, 4),
                    reason: Some("The dog ate my command".to_string()),
                },
                method: DesiredFeatureMethod::Manual,
                mode: DesiredMode::Sync,
            },
        );
        thing.desired_state.insert(
            "method_code".to_string(),
            DesiredFeature {
                last_update: Utc.ymd(2022, 1, 1).and_hms(1, 2, 3),
                valid_until: None,
                value: json!(42),
                reconciliation: DesiredFeatureReconciliation::Reconciling {
                    last_attempt: Some(Utc.ymd(2022, 1, 1).and_hms(1, 2, 4)),
                },
                method: DesiredFeatureMethod::Code(Code::JavaScript("true".to_string())),
                mode: DesiredMode::Sync,
            },
        );
        thing.desired_state.insert(
            "method_command".to_string(),
            DesiredFeature {
                last_update: Utc.ymd(2022, 1, 1).and_hms(1, 2, 3),
                valid_until: None,
                value: json!(42),
                reconciliation: DesiredFeatureReconciliation::Reconciling {
                    last_attempt: Some(Utc.ymd(2022, 1, 1).and_hms(1, 2, 4)),
                },
                method: DesiredFeatureMethod::Command {
                    channel: "set-feature".to_string(),
                    payload: DesiredFeatureCommandPayload::Raw,
                },
                mode: DesiredMode::Sync,
            },
        );
        thing.desired_state.insert(
            "method_external".to_string(),
            DesiredFeature {
                last_update: Utc.ymd(2022, 1, 1).and_hms(1, 2, 3),
                valid_until: None,
                value: json!(42),
                reconciliation: DesiredFeatureReconciliation::Reconciling {
                    last_attempt: Some(Utc.ymd(2022, 1, 1).and_hms(1, 2, 4)),
                },
                method: DesiredFeatureMethod::External,
                mode: DesiredMode::Sync,
            },
        );
        assert_eq!(
            json!({
                "metadata": {
                    "name": "thing",
                    "application": "app",
                },
                "desiredState": {
                    "bar": {
                        "value": 42,
                        "lastUpdate": "2022-01-01T01:02:03Z",
                        "reconciliation": {
                            "state": "succeeded",
                            "when": "2022-01-01T01:02:04Z",
                        },
                        "method": "manual",
                    },
                    "baz": {
                        "value": 42,
                        "lastUpdate": "2022-01-01T01:02:03Z",
                        "reconciliation": {
                            "state": "failed",
                            "when": "2022-01-01T01:02:04Z",
                            "reason": "The dog ate my command",
                        },
                        "method": "manual",
                    },
                    "foo": {
                        "value": 42,
                        "lastUpdate": "2022-01-01T01:02:03Z",
                        "validUntil": "2022-01-01T02:02:03Z",
                        "reconciliation": {
                            "state": "reconciling",
                            "lastAttempt": "2022-01-01T01:02:04Z",
                        },
                        "method": "manual",
                    },
                    "method_code": {
                        "value": 42,
                        "lastUpdate": "2022-01-01T01:02:03Z",
                        "reconciliation": {
                            "state": "reconciling",
                            "lastAttempt": "2022-01-01T01:02:04Z",
                        },
                        "method": {
                            "code": {
                                "javaScript": "true",
                            },
                        },
                    },
                    "method_command": {
                        "value": 42,
                        "lastUpdate": "2022-01-01T01:02:03Z",
                        "reconciliation": {
                            "state": "reconciling",
                            "lastAttempt": "2022-01-01T01:02:04Z",
                        },
                        "method": {
                            "command": {
                                "channel": "set-feature",
                                "payload": "raw",
                            }
                        },
                    },
                    "method_external": {
                        "value": 42,
                        "lastUpdate": "2022-01-01T01:02:03Z",
                        "reconciliation": {
                            "state": "reconciling",
                            "lastAttempt": "2022-01-01T01:02:04Z",
                        },
                        "method": "external",
                    },
                }
            }),
            serde_json::to_value(thing).unwrap()
        );
    }
}
