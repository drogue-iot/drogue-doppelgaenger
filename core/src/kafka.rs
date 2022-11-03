use opentelemetry::propagation::Injector;
use rdkafka::message::{Header, OwnedHeaders, ToBytes};

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

pub trait AddHeader {
    fn add<K: AsRef<str>, V: ToBytes + ?Sized>(self, key: K, value: &V) -> Self;
}

impl AddHeader for OwnedHeaders {
    fn add<K: AsRef<str>, V: ToBytes + ?Sized>(self, key: K, value: &V) -> Self {
        self.insert(Header {
            key: key.as_ref(),
            value: Some(value),
        })
    }
}
