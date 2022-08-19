use crate::injector::metadata::Meta;
use crate::processor::Message;
use anyhow::{anyhow, bail};
use cloudevents::Data;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use url::Url;

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum PayloadMapper {
    #[serde(alias = "raw")]
    /// Expects the payload to be the Drogue Doppelgaenger schema
    Raw,
    #[serde(alias = "simpleJson")]
    /// Expects a JSON object as payload, taking the root level properties as reported state properties.
    SimpleJson {
        /// If the data is a partial update.
        #[serde(default)]
        partial: bool,
        /// Add a timestamp value from the source event.
        #[serde(default)]
        add_timestamp: bool,
    },
    #[serde(alias = "simpleState")]
    SimpleState,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SimpleState {
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub state: BTreeMap<String, Value>,
}

impl Default for PayloadMapper {
    fn default() -> Self {
        Self::Raw
    }
}

impl PayloadMapper {
    pub fn map(
        &self,
        meta: &Meta,
        data: (Option<String>, Option<Url>, Option<Data>),
    ) -> anyhow::Result<Message> {
        match self {
            Self::Raw => self.map_raw(data),
            Self::SimpleJson {
                partial,
                add_timestamp,
            } => self.map_simple(&meta, data, *partial, *add_timestamp),
            Self::SimpleState => self.map_simple_state(data),
        }
    }

    fn map_raw(
        &self,
        (content_type, schema, data): (Option<String>, Option<Url>, Option<Data>),
    ) -> anyhow::Result<Message> {
        match (content_type.as_deref(), schema, data) {
            (Some("application/vnd.drogue-iot.doppelgaenger.message+json"), _, Some(data)) => {
                Ok(from_data(data)?)
            }
            (content_type, schema, data) => self.otherwise(content_type, schema, data),
        }
    }

    fn map_simple(
        &self,
        meta: &Meta,
        (content_type, schema, data): (Option<String>, Option<Url>, Option<Data>),
        partial: bool,
        add_timestamp: bool,
    ) -> anyhow::Result<Message> {
        match (content_type.as_deref(), schema, data) {
            (Some("application/json" | "text/json"), _, Some(data)) => match from_data(data)? {
                Value::Object(props) => {
                    let mut state: BTreeMap<String, Value> = props.into_iter().collect();
                    if add_timestamp {
                        state.insert(
                            "lastMessage".to_string(),
                            meta.timestamp.to_rfc3339().into(),
                        );
                    }
                    Ok(Message::ReportState { state, partial })
                }
                _ => {
                    bail!("Wrong root level value for {self:?} mapper, expected: Object");
                }
            },
            (content_type, schema, data) => self.otherwise(content_type, schema, data),
        }
    }

    fn map_simple_state(
        &self,
        (content_type, schema, data): (Option<String>, Option<Url>, Option<Data>),
    ) -> anyhow::Result<Message> {
        match (content_type.as_deref(), schema, data) {
            (Some("application/json" | "text/json"), _, Some(data)) => {
                let SimpleState { state, partial } = from_data::<SimpleState>(data)?;
                Ok(Message::ReportState { state, partial })
            }
            (content_type, schema, data) => self.otherwise(content_type, schema, data),
        }
    }

    fn otherwise(
        &self,
        content_type: Option<&str>,
        schema: Option<Url>,
        data: Option<Data>,
    ) -> anyhow::Result<Message> {
        log::warn!(
            "Unknown payload for mapper: {:?} - contentType: {}, schema: {}, data: {data:?}",
            self,
            content_type.unwrap_or("<none>"),
            schema.as_ref().map(|s| s.as_str()).unwrap_or("<none>")
        );
        Err(anyhow!("Unknown payload"))
    }
}

fn from_data<T>(data: Data) -> Result<T, serde_json::Error>
where
    for<'de> T: Deserialize<'de>,
{
    match data {
        Data::Json(value) => serde_json::from_value(value),
        Data::String(string) => serde_json::from_str(&string),
        Data::Binary(blob) => serde_json::from_slice(&blob),
    }
}
