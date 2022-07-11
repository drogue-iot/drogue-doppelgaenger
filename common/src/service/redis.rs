use crate::model::{DesiredFeature, ReportedFeature, SyntheticFeature, Thing};
use crate::service;
use async_trait::async_trait;
use deadpool_redis::{PoolError, Runtime};
use redis::AsyncCommands;
use std::collections::BTreeMap;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub redis: deadpool_redis::Config,
}

/// The persisted state
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct State {
    pub reported_state: BTreeMap<String, ReportedFeature>,
    pub desired_state: BTreeMap<String, DesiredFeature>,
    pub synthetic_state: BTreeMap<String, SyntheticFeature>,
}

pub struct Service {
    pool: deadpool_redis::Pool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("Pool error: {0}")]
    Pool(#[from] PoolError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("{0}")]
    Generic(String),
}

impl From<Error> for service::Error<Error> {
    fn from(err: Error) -> Self {
        service::Error::Internal(err)
    }
}

type Result<T> = std::result::Result<T, service::Error<Error>>;

#[async_trait]
impl super::Service for Service {
    type Config = Config;
    type Error = Error;

    fn new(config: Self::Config) -> anyhow::Result<Self> {
        let pool = config.redis.create_pool(Some(Runtime::Tokio1))?;
        Ok(Self { pool })
    }

    async fn get(&self, name: &str) -> Result<Thing> {
        let mut con = self.pool.get().await.map_err(Error::Pool)?;

        let thing: Option<Vec<u8>> = con.get(name).await.map_err(Error::Redis)?;

        match thing {
            Some(thing) => Ok(serde_json::from_slice(&thing).map_err(Error::Serialization)?),
            None => Err(service::Error::NotFound),
        }
    }

    async fn create(&self, thing: Thing) -> Result<()> {
        let mut con = self.pool.get().await.map_err(Error::Pool)?;

        let name = thing.metadata.name.clone();
        log::info!("Creating new thing: {name}");

        let result: Option<String> = redis::cmd("SET")
            .arg(name)
            .arg(serde_json::to_string(&thing)?)
            .arg("NX")
            .query_async(&mut con)
            .await
            .map_err(Error::Redis)?;

        match result.as_deref() {
            None => Err(service::Error::AlreadyExists),
            Some("OK") => Ok(()),
            _ => Err(Error::Generic("Invalid result for create operation".into()))?,
        }
    }

    async fn update(&self, thing: Thing) -> Result<()> {
        let mut con = self.pool.get().await.map_err(Error::Pool)?;

        let name = thing.metadata.name.clone();
        log::info!("Updating existing thing: {name}");

        let result: Option<String> = redis::cmd("SET")
            .arg(name)
            .arg(serde_json::to_string(&thing)?)
            .arg("XX")
            .query_async(&mut con)
            .await
            .map_err(Error::Redis)?;

        match result.as_deref() {
            None => Err(service::Error::NotFound),
            Some("OK") => Ok(()),
            _ => Err(Error::Generic("Invalid result for update operation".into()))?,
        }
    }

    async fn delete(&self, name: &str) -> Result<()> {
        let mut con = self.pool.get().await.map_err(Error::Pool)?;

        log::info!("Deleting thing: {name}");

        con.del(name).await.map_err(Error::Redis)?;

        Ok(())
    }
}
