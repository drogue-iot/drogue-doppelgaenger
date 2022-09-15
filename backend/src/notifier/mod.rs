pub mod actix;

use chrono::{DateTime, Utc};
use drogue_doppelgaenger_core::processor;
use drogue_doppelgaenger_model::Thing;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum SetDesiredValue {
    #[serde(rename_all = "camelCase")]
    WithOptions {
        value: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        valid_until: Option<DateTime<Utc>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[serde(with = "humantime_serde")]
        valid_for: Option<Duration>,
    },
    Value(Value),
}

#[derive(Debug, thiserror::Error)]
pub enum SetDesiredValueError {
    #[error("Duration out of range: {0}")]
    OutOfRange(#[from] time::OutOfRangeError),
    #[error("Invalid combination: only one of valid for or until must be present")]
    Invalid,
}

impl TryFrom<SetDesiredValue> for processor::SetDesiredValue {
    type Error = SetDesiredValueError;

    fn try_from(value: SetDesiredValue) -> Result<Self, Self::Error> {
        Ok(match value {
            SetDesiredValue::Value(value) => Self::Value(value),
            SetDesiredValue::WithOptions {
                value,
                valid_until,
                valid_for: None,
            } => Self::WithOptions { value, valid_until },
            SetDesiredValue::WithOptions {
                value,
                valid_until: None,
                valid_for: Some(valid_for),
            } => {
                let valid_until = Utc::now() + chrono::Duration::from_std(valid_for)?;
                Self::WithOptions {
                    value,
                    valid_until: Some(valid_until),
                }
            }
            SetDesiredValue::WithOptions {
                value: _,
                valid_until: Some(_),
                valid_for: Some(_),
            } => return Err(SetDesiredValueError::Invalid),
        })
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Request {
    Subscribe {
        thing: String,
    },
    Unsubscribe {
        thing: String,
    },
    SetDesiredValues {
        thing: String,
        values: BTreeMap<String, SetDesiredValue>,
    },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Response {
    Initial { thing: Arc<Thing> },
    Change { thing: Arc<Thing> },
    Lag { lag: u64 },
}
