use super::*;
use chrono::{DateTime, Utc};
use serde_json::Value;

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct DesiredFeature {
    /// The value the system desired the device to apply. If the value is not set, then nothing will
    /// be reconciled.
    #[serde(default)]
    pub value: Value,
    /// The mode this value should be applied with.
    #[serde(default, skip_serializing_if = "crate::is_default")]
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
    /// Reconciliation is disabled.
    Disabled,
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
    Succeeded {
        when: DateTime<Utc>,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        when: DateTime<Utc>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    Disabled {
        when: DateTime<Utc>,
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
    Command(Command),
    /// Generate reconcile actions through custom code.
    Code(Code),
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Command {
    /// The period when a command will be sent out to reconcile.
    #[serde(with = "humantime_serde")]
    #[schemars(schema_with = "crate::types::humantime")]
    pub period: std::time::Duration,

    /// If the command should be sent actively after the period expired.
    #[serde(default)]
    pub mode: CommandMode,

    /// The encoding of the command
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<CommandEncoding>,
}

#[derive(
    Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum CommandMode {
    Active,
    #[default]
    Passive,
}

#[derive(
    Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum CommandEncoding {
    Remap { device: String, channel: String },
    // Send the desired value as part of a map, using the feature name as key. Combine with other
    // value sent to the same channel.
    Channel(String),
    // Send the desired value as payload, using the feature name as channel.
    Raw,
}

#[cfg(test)]
mod test {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn test_ser_desired() {
        let mut thing: Thing = Thing::new("app", "thing");
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
                method: DesiredFeatureMethod::Command(Command {
                    encoding: Some(CommandEncoding::Channel("set-features".to_string())),
                    period: std::time::Duration::from_secs(30),
                    mode: CommandMode::Passive,
                }),
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
                                "period": "30s",
                                "encoding": {
                                    "channel": "set-features",
                                },
                                "mode": "passive",
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
