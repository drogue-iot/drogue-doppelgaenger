use std::collections::HashMap;

pub struct KafkaProperties(pub HashMap<String, String>);

impl KafkaProperties {
    pub fn new<I>(i: I) -> Self
    where
        I: IntoIterator<Item = HashMap<String, String>>,
    {
        let mut map = HashMap::new();

        for props in i.into_iter() {
            map.extend(props);
        }

        Self(map)
    }
}

impl From<KafkaProperties> for rdkafka::ClientConfig {
    fn from(properties: KafkaProperties) -> Self {
        let mut result = rdkafka::ClientConfig::new();

        for (k, v) in properties.0 {
            result.set(k.replace('_', "."), v);
        }

        result
    }
}
