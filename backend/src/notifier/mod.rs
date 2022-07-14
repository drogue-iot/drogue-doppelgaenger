pub mod actix;

use drogue_doppelgaenger_core::model::Thing;
use drogue_doppelgaenger_core::service::Id;
use std::sync::Arc;
use std::time::Duration;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Request {
    Subscribe(Id),
    Unsubscribe(Id),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "type")]
pub enum Response {
    Change { thing: Arc<Thing> },
    Lag { lag: u64 },
}
