use super::*;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use std::time::Duration;

#[derive(
    Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub struct Reconciliation {
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub changed: IndexMap<String, Changed>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub timers: IndexMap<String, Timer>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub deleting: IndexMap<String, Deleting>,
}

impl Reconciliation {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.timers.is_empty() && self.deleting.is_empty()
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
    pub period: Duration,
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
    pub initial_delay: Option<Duration>,
}

impl Timer {
    pub fn new(period: Duration, initial_delay: Option<Duration>, code: Code) -> Self {
        Self {
            code,
            last_log: Default::default(),
            period,
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
pub struct Deleting {
    #[serde(flatten)]
    /// Code that will be executed before the thing will be deleted.
    pub code: Code,
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    pub fn test_ser() {
        let mut thing = Thing::new("default", "thing1");
        thing.reconciliation.deleting.insert(
            "test".to_string(),
            Deleting {
                code: Code::JavaScript("other code".to_string()),
            },
        );

        assert_eq!(
            json!({
                "metadata": {
                    "application": "default",
                    "name": "thing1"
                },
                "reconciliation": {
                    "deleting": {
                        "test": {
                            "javaScript": "other code",
                        },
                    }
                }
            }),
            serde_json::to_value(thing).unwrap()
        );
    }
}
