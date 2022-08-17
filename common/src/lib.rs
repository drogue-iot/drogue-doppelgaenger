use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct KafkaConfig {
    pub properties: HashMap<String, String>,
    pub topic: String,
}
