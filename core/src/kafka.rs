use opentelemetry::propagation::Injector;
use rdkafka::message::OwnedHeaders;

pub struct KafkaHeaders(Option<OwnedHeaders>);

impl Injector for KafkaHeaders {
    fn set(&mut self, key: &str, value: String) {
        let h = self.0.take().unwrap_or_default();
        self.0 = Some(h.add(&format!("ce_{}", key), &value))
    }
}

impl From<OwnedHeaders> for KafkaHeaders {
    fn from(headers: OwnedHeaders) -> Self {
        Self(Some(headers))
    }
}

impl From<KafkaHeaders> for OwnedHeaders {
    fn from(headers: KafkaHeaders) -> Self {
        headers.0.unwrap_or_default()
    }
}
