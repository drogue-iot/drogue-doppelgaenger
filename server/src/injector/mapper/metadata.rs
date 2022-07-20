use anyhow::anyhow;
use chrono::{DateTime, Utc};
use cloudevents::AttributesReader;

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "type")]
pub enum MetadataMapper {
    #[serde(alias = "raw")]
    /// Expects the payload to the the Drogue Doppelgaenger schema
    Raw {
        #[serde(default)]
        override_application: Option<String>,
    },
}

impl Default for MetadataMapper {
    fn default() -> Self {
        Self::Raw {
            override_application: None,
        }
    }
}

pub struct Meta {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub application: String,
    pub device: String,
}

impl MetadataMapper {
    pub fn map(&self, event: &cloudevents::Event) -> anyhow::Result<Option<Meta>> {
        match self {
            Self::Raw {
                override_application,
            } => Self::map_raw(event, override_application.as_deref()),
        }
    }

    fn map_raw(
        event: &cloudevents::Event,
        override_application: Option<&str>,
    ) -> anyhow::Result<Option<Meta>> {
        if event.ty() != "io.drogue.event.v1" {
            return Ok(None);
        }

        let application = match override_application {
            Some(application) => application.to_string(),
            None => event
                .extension("application")
                .ok_or_else(|| anyhow!("Missing 'application' extension value"))?
                .to_string(),
        };

        let device = event
            .extension("device")
            .ok_or_else(|| anyhow!("Missing 'device' extension value"))?
            .to_string();

        let id = event.id().to_string();
        let timestamp = event.time().map(|d| *d).unwrap_or_else(|| Utc::now());

        Ok(Some(Meta {
            id,
            timestamp,
            application,
            device,
        }))
    }
}
