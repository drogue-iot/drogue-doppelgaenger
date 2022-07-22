use crate::model::WakerReason;
use crate::service::Id;
use crate::waker::TargetId;
use anyhow::{anyhow, bail};
use async_trait::async_trait;
use deadpool_postgres::{Client, Runtime, Transaction};
use postgres_types::{Json, Type};
use std::future::Future;
use std::time::Duration;
use tokio::time::MissedTickBehavior;
use tokio_postgres::Statement;
use uuid::Uuid;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub application: Option<String>,
    pub postgres: deadpool_postgres::Config,
}

pub struct Waker {
    application: Option<String>,
    pool: deadpool_postgres::Pool,
}

#[async_trait]
impl super::Waker for Waker {
    type Config = Config;

    fn from_config(config: Self::Config) -> anyhow::Result<Self> {
        let pool = config.postgres.create_pool(
            Some(Runtime::Tokio1),
            postgres_native_tls::MakeTlsConnector::new(native_tls::TlsConnector::new()?),
        )?;

        Ok(Self {
            pool,
            application: config.application,
        })
    }

    async fn run<F, Fut>(self, f: F) -> anyhow::Result<()>
    where
        F: Fn(TargetId, Vec<WakerReason>) -> Fut + Send + Sync,
        Fut: Future<Output = anyhow::Result<()>> + Send,
    {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let stmt = self.build_statement();

        loop {
            interval.tick().await;
            log::debug!("Ticking ...");

            match self.pool.get().await {
                Err(err) => {
                    // FIXME: map to liveness status
                    log::warn!("Failed to prepare for tick: {err}");
                }
                Ok(con) => {
                    if let Err(err) = WakerRun::new(con, &stmt, &self.application, &f).run().await {
                        // FIXME: map to liveness status
                        log::warn!("Failed to tick: {err}");
                    }
                }
            }
        }
    }
}

impl Waker {
    fn build_statement(&self) -> (String, Vec<Type>) {
        let mut types = vec![];

        let and_application = match self.application.is_some() {
            true => {
                types.push(Type::VARCHAR);
                r#"
            AND
                APPLICATION = $1
"#
            }
            false => "",
        };

        // We retrieve the next thing, and lock it for an update. We only fetch one, and skip
        // all locked rows. So we can scale up processing to some degree. Once we successfully
        // scheduled the wakeup (e.g. sending that to Kafka) we can update the record and commit.

        let stmt = format!(
            r#"
SELECT
    APPLICATION,
    NAME,
    UID,
    RESOURCE_VERSION,
    WAKER_REASONS

FROM
    things

WHERE
        WAKER <= NOW()
{and_application}

ORDER BY
    WAKER ASC 

LIMIT 1
FOR UPDATE SKIP LOCKED
"#
        );

        (stmt, types)
    }
}

struct WakerRun<'r, F, Fut>
where
    F: Fn(TargetId, Vec<WakerReason>) -> Fut + Send,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    con: Client,
    stmt: &'r (String, Vec<Type>),
    application: &'r Option<String>,

    f: &'r F,
}

impl<'r, F, Fut> WakerRun<'r, F, Fut>
where
    F: Fn(TargetId, Vec<WakerReason>) -> Fut + Send,
    Fut: Future<Output = anyhow::Result<()>> + Send,
{
    fn new(
        con: Client,
        stmt: &'r (String, Vec<Type>),
        application: &'r Option<String>,

        f: &'r F,
    ) -> Self {
        Self {
            con,
            stmt,
            application,

            f,
        }
    }

    async fn run(mut self) -> anyhow::Result<()> {
        let stmt = self
            .con
            .prepare_typed_cached(&self.stmt.0, &self.stmt.1)
            .await?;

        loop {
            if !self.tick_next(&stmt).await? {
                log::debug!("No more for this time");
                break;
            }
        }

        Ok(())
    }

    async fn tick_next(&mut self, stmt: &Statement) -> anyhow::Result<bool> {
        let tx = self.con.build_transaction().start().await?;

        let row = match &self.application {
            Some(application) => tx.query_opt(stmt, &[application]).await,
            None => tx.query_opt(stmt, &[]).await,
        }?;

        Ok(match row {
            Some(row) => {
                let application: String = row.try_get("APPLICATION")?;
                let thing: String = row.try_get("NAME")?;
                let uid: Uuid = row.try_get("UID")?;
                let resource_version: Uuid = row.try_get("RESOURCE_VERSION")?;
                let reasons: Vec<WakerReason> =
                    row.try_get::<_, Json<Vec<WakerReason>>>("WAKER_REASONS")?.0;

                // send wakeup

                log::debug!("Wakeup: {application} / {thing} / {uid}");
                (self.f)(
                    TargetId {
                        id: Id {
                            application: application.clone(),
                            thing: thing.clone(),
                        },
                        uid: uid.to_string(),
                        resource_version: resource_version.to_string(),
                    },
                    reasons,
                )
                .await?;

                // clear waker

                Self::clear_waker(tx, application, thing, uid, resource_version).await?;

                // done with this entry

                true
            }
            None => false,
        })
    }

    async fn clear_waker(
        tx: Transaction<'_>,
        application: String,
        thing: String,
        uid: Uuid,
        resource_version: Uuid,
    ) -> anyhow::Result<()> {
        // we clear the waker and commit the transaction. The oplock should hold, as we have locked
        // the record.

        let stmt = tx
            .prepare_typed_cached(
                r#"
UPDATE
    things
SET
    WAKER = NULL, WAKER_REASONS = NULL
WHERE
        APPLICATION = $1
    AND
        NAME = $2
    AND
        UID = $3
    AND
        RESOURCE_VERSION = $4
"#,
                &[Type::VARCHAR, Type::VARCHAR, Type::UUID, Type::UUID],
            )
            .await?;

        let result = tx
            .execute(&stmt, &[&application, &thing, &uid, &resource_version])
            .await?;

        if result == 0 {
            bail!("Lost oplock during waking.");
        }

        tx.commit().await?;

        Ok(())
    }
}
