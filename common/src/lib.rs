use std::collections::HashMap;

pub mod stream;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct KafkaConfig {
    pub properties: HashMap<String, String>,
    pub topic: String,
}
