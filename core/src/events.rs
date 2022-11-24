use cloudevents::Data;
use serde::de::DeserializeOwned;

/// Work with the data section of cloud events.
pub trait DataExt {
    /// Take the data, as JSON.
    ///
    /// If the data section is not present, or contains an empty value, [`None`] is returned.
    fn take_as_json<T: DeserializeOwned>(&mut self) -> Result<Option<T>, serde_json::Error>;
}

impl DataExt for cloudevents::Event {
    fn take_as_json<T: DeserializeOwned>(&mut self) -> Result<Option<T>, serde_json::Error> {
        let (_content_type, _schema, data) = self.take_data();

        match data {
            Some(Data::String(s)) => {
                if s.is_empty() {
                    return Ok(None);
                }
                Some(serde_json::from_str(&s)).transpose()
            }
            Some(Data::Json(v)) => {
                if v.is_null() {
                    return Ok(None);
                }
                Some(serde_json::from_value(v)).transpose()
            }
            Some(Data::Binary(b)) => {
                if b.is_empty() {
                    return Ok(None);
                }
                Some(serde_json::from_slice(&b)).transpose()
            }
            None => Ok(None),
        }
    }
}
