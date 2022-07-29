mod utils;

use crate::{
    model::{
        DesiredFeature, Internal, Metadata, Reconciliation, ReportedFeature, Schema,
        SyntheticFeature, Thing,
    },
    storage::{self},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use deadpool_postgres::{PoolError, Runtime};
use postgres_types::Type;
use std::collections::BTreeMap;
use tokio_postgres::{
    error::SqlState,
    types::{Json, ToSql},
    Row,
};
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub application: Option<String>,
    pub postgres: deadpool_postgres::Config,
}

pub struct ThingEntity {
    pub uid: Uuid,
    pub creation_timestamp: DateTime<Utc>,
    pub generation: u32,
    pub resource_version: Uuid,

    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,

    pub data: Data,

    pub waker: Option<DateTime<Utc>>,
}

/// The persisted data field
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Data {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Schema>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reported_state: BTreeMap<String, ReportedFeature>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub desired_state: BTreeMap<String, DesiredFeature>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub synthetic_state: BTreeMap<String, SyntheticFeature>,

    #[serde(default, skip_serializing_if = "Reconciliation::is_empty")]
    pub reconciliation: Reconciliation,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub internal: Option<Internal>,
}

impl From<&Thing> for Data {
    fn from(value: &Thing) -> Self {
        Self {
            schema: value.schema.clone(),
            reported_state: value.reported_state.clone(),
            desired_state: value.desired_state.clone(),
            synthetic_state: value.synthetic_state.clone(),
            reconciliation: value.reconciliation.clone(),
            internal: value.internal.clone(),
        }
    }
}

impl TryFrom<Row> for ThingEntity {
    type Error = Error;

    fn try_from(row: Row) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            uid: row.try_get("UID")?,
            creation_timestamp: row.try_get("CREATION_TIMESTAMP")?,

            generation: row.try_get::<_, i64>("GENERATION")? as u32,
            resource_version: row.try_get("RESOURCE_VERSION")?,
            labels: utils::row_to_map(&row, "LABELS")?,
            annotations: utils::row_to_map(&row, "ANNOTATIONS")?,
            data: row.try_get::<_, Json<_>>("DATA")?.0,

            waker: row.try_get("WAKER")?,
        })
    }
}

pub struct Storage {
    application: Option<String>,
    pool: deadpool_postgres::Pool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Postgres error: {0}")]
    Postgres(#[from] tokio_postgres::Error),
    #[error("Pool error: {0}")]
    Pool(#[from] PoolError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("{0}")]
    Generic(String),
}

impl From<Error> for storage::Error<Error> {
    fn from(err: Error) -> Self {
        storage::Error::Internal(err)
    }
}

type Result<T> = std::result::Result<T, storage::Error<Error>>;

#[async_trait]
impl super::Storage for Storage {
    type Config = Config;
    type Error = Error;

    fn from_config(config: &Self::Config) -> anyhow::Result<Self> {
        let pool = config.postgres.create_pool(
            Some(Runtime::Tokio1),
            postgres_native_tls::MakeTlsConnector::new(native_tls::TlsConnector::new()?),
        )?;
        let application = config.application.clone();
        Ok(Self { application, pool })
    }

    async fn get(&self, application: &str, name: &str) -> Result<Option<Thing>> {
        if let Err(storage::Error::NotFound) =
            self.ensure_app(application, || storage::Error::NotFound)
        {
            return Ok(None);
        }

        let con = self.pool.get().await.map_err(Error::Pool)?;

        let stmt = con
            .prepare_typed_cached(
                r#"
SELECT
    UID,
    CREATION_TIMESTAMP,
    GENERATION,
    RESOURCE_VERSION,
    ANNOTATIONS,
    LABELS,
    DATA,
    WAKER
FROM
    THINGS
WHERE
        NAME = $1
    AND
        APPLICATION = $2 
"#,
                &[
                    Type::VARCHAR, // name
                    Type::VARCHAR, // application
                ],
            )
            .await
            .map_err(Error::Postgres)?;

        match con
            .query_opt(&stmt, &[&name, &application])
            .await
            .map_err(Error::Postgres)?
        {
            Some(row) => {
                let entity: ThingEntity = row.try_into()?;
                Ok(Some(Thing {
                    metadata: Metadata {
                        name: name.to_string(),
                        application: application.to_string(),
                        uid: Some(entity.uid.to_string()),
                        creation_timestamp: Some(entity.creation_timestamp),
                        resource_version: Some(entity.resource_version.to_string()),

                        generation: Some(entity.generation),
                        annotations: entity.annotations,
                        labels: entity.labels,
                    },
                    schema: entity.data.schema,
                    reported_state: entity.data.reported_state,
                    desired_state: entity.data.desired_state,
                    synthetic_state: entity.data.synthetic_state,
                    reconciliation: entity.data.reconciliation,
                    internal: entity.data.internal,
                }))
            }
            None => Err(storage::Error::NotFound),
        }
    }

    async fn create(&self, mut thing: Thing) -> Result<Thing> {
        self.ensure_app(&thing.metadata.application, || storage::Error::NotAllowed)?;

        let con = self.pool.get().await.map_err(Error::Pool)?;

        // Init metadata. We need to set this on the thing too, as we return it.
        let uid = Uuid::new_v4();
        let resource_version = Uuid::new_v4();
        let generation = 1i64;
        let creation_timestamp = Utc::now();
        thing.metadata.uid = Some(uid.to_string());
        thing.metadata.creation_timestamp = Some(creation_timestamp);
        thing.metadata.generation = Some(generation as u32);
        thing.metadata.resource_version = Some(resource_version.to_string());

        let waker = waker_data(&thing);

        log::debug!(
            "Creating new thing: {} / {}",
            thing.metadata.application,
            thing.metadata.name
        );

        let data: Data = (&thing).into();

        let stmt = con
            .prepare_typed_cached(
                r#"
INSERT INTO things (
    NAME,
    APPLICATION,
    UID,
    CREATION_TIMESTAMP,
    GENERATION,
    RESOURCE_VERSION,
    ANNOTATIONS,
    LABELS,
    DATA,
    WAKER
) VALUES (
    $1,
    $2,
    $3,
    $4,
    $5,
    $6,
    $7,
    $8,
    $9,
    $10
)
"#,
                &[
                    Type::VARCHAR,     // name
                    Type::VARCHAR,     // application
                    Type::UUID,        // uid
                    Type::TIMESTAMPTZ, // creation timestamp
                    Type::INT8,        // generation
                    Type::UUID,        // resource version
                    Type::JSON,        // annotations
                    Type::JSONB,       // labels
                    Type::JSON,        // data
                    Type::TIMESTAMPTZ, // waker
                ],
            )
            .await
            .map_err(Error::Postgres)?;

        con.execute(
            &stmt,
            &[
                &thing.metadata.name,
                &thing.metadata.application,
                &uid,
                &Utc::now(),
                &generation,
                &resource_version,
                &Json(&thing.metadata.annotations),
                &Json(&thing.metadata.labels),
                &Json(data),
                &waker,
            ],
        )
        .await
        .map_err(|err| match err.code() {
            Some(&SqlState::UNIQUE_VIOLATION) => storage::Error::AlreadyExists,
            _ => Error::Postgres(err).into(),
        })?;

        Ok(thing.clone())
    }

    async fn update(&self, mut thing: Thing) -> Result<Thing> {
        self.ensure_app(&thing.metadata.application, || storage::Error::NotFound)?;

        let con = self.pool.get().await.map_err(Error::Pool)?;

        let name = &thing.metadata.name;
        let application = &thing.metadata.application;

        let waker = waker_data(&thing);

        log::debug!("Updating existing thing: {application} / {name}");

        let mut stmt = r#"
UPDATE things
SET
    GENERATION = GENERATION + 1,
    RESOURCE_VERSION = $3,
    ANNOTATIONS = $4,
    LABELS = $5,
    DATA = $6,
    WAKER = $7
WHERE
        NAME = $1
    AND
        APPLICATION = $2
"#
        .to_string();

        let resource_version = Uuid::new_v4();
        let data = Json(Data::from(&thing));
        let annotations = Json(&thing.metadata.annotations);
        let labels = Json(&thing.metadata.labels);

        let mut types: Vec<Type> = Vec::new();
        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
        types.push(Type::VARCHAR);
        params.push(name);
        types.push(Type::VARCHAR);
        params.push(application);
        types.push(Type::UUID);
        params.push(&resource_version);
        types.push(Type::JSON);
        params.push(&annotations);
        types.push(Type::JSONB);
        params.push(&labels);
        types.push(Type::JSON);
        params.push(&data);
        types.push(Type::TIMESTAMPTZ);
        params.push(&waker);

        if let Some(resource_version) = &thing.metadata.resource_version {
            stmt.push_str(&format!(
                "    AND RESOURCE_VERSION::text=${}",
                params.len() + 1
            ));
            types.push(Type::TEXT);
            params.push(resource_version);
        }
        if let Some(uid) = &thing.metadata.uid {
            stmt.push_str(&format!("    AND UID::text=${}", params.len() + 1));
            types.push(Type::TEXT);
            params.push(uid);
        }

        stmt.push_str(
            r#"
RETURNING
    CREATION_TIMESTAMP, GENERATION, UID::text
"#,
        );

        let stmt = con
            .prepare_typed_cached(&stmt, &types)
            .await
            .map_err(Error::Postgres)?;

        match con.query_opt(&stmt, &params).await {
            Ok(None) => Err(storage::Error::PreconditionFailed),
            Ok(Some(row)) => {
                // update metadata, with new values

                thing.metadata.uid = row.try_get("UID").map_err(Error::Postgres)?;
                thing.metadata.creation_timestamp =
                    row.try_get("CREATION_TIMESTAMP").map_err(Error::Postgres)?;
                thing.metadata.generation = Some(
                    row.try_get::<_, i64>("GENERATION")
                        .map_err(Error::Postgres)? as u32,
                );
                thing.metadata.resource_version = Some(resource_version.to_string());

                Ok(thing)
            }
            Err(err) => Err(Error::Postgres(err).into()),
        }
    }

    async fn delete(&self, application: &str, name: &str) -> Result<bool> {
        if let Err(storage::Error::NotFound) =
            self.ensure_app(application, || storage::Error::NotFound)
        {
            return Ok(false);
        }

        let con = self.pool.get().await.map_err(Error::Pool)?;

        log::debug!("Deleting thing: {application} / {name}");

        let stmt = con
            .prepare_typed_cached(
                r#"
DELETE FROM things
WHERE
        NAME = $1
    AND
        APPLICATION = $2
"#,
                &[
                    Type::VARCHAR, // name
                    Type::VARCHAR, // application
                ],
            )
            .await
            .map_err(Error::Postgres)?;

        // FIXME: add uid and resource version

        let rows = con
            .execute(&stmt, &[&name, &application])
            .await
            .map_err(Error::Postgres)?;

        Ok(rows > 0)
    }
}

impl Storage {
    fn ensure_app<F, E>(&self, application: &str, f: F) -> Result<()>
    where
        F: FnOnce() -> E,
        E: Into<storage::Error<Error>>,
    {
        if let Some(expected_application) = &self.application {
            if expected_application != application {
                return Err(f().into());
            }
        }
        Ok(())
    }
}

fn waker_data(thing: &Thing) -> Option<DateTime<Utc>> {
    thing.internal.as_ref().and_then(|i| i.waker.when)
}
