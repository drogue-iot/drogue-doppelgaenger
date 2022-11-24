use drogue_doppelgaenger_model::{InternalState, Thing};
use indexmap::IndexMap;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TwinProperties {
    pub desired: IndexMap<String, serde_json::Value>,
    pub reported: IndexMap<String, serde_json::Value>,
}

impl<I: InternalState> From<Thing<I>> for TwinProperties {
    fn from(thing: Thing<I>) -> Self {
        Self {
            desired: thing
                .desired_state
                .into_iter()
                .map(|(k, v)| (k, v.value))
                .collect(),
            reported: thing
                .reported_state
                .into_iter()
                .map(|(k, v)| (k, v.value))
                .collect(),
        }
    }
}
