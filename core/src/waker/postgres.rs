use crate::model::WakerReason;
use crate::service::Id;
use crate::waker::TargetId;
use async_trait::async_trait;
use deadpool_postgres::{Client, Runtime};
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
    #[serde(with = "humantime_serde", default = "default::waker_delay")]
    pub waker_delay: Duration,
}

mod default {
    use std::time::Duration;

    pub const fn waker_delay() -> Duration {
        Duration::from_secs(1)
    }
}

pub struct Waker {
    application: Option<String>,
    pool: deadpool_postgres::Pool,
    // waker delay in ISO 8609 duration format
    waker_delay: String,
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
            waker_delay: chrono::Duration::from_std(config.waker_delay)?.to_string(),
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
                    if let Err(err) =
                        WakerRun::new(con, &stmt, &self.application, &self.waker_delay, &f)
                            .run()
                            .await
                    {
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
        let mut types = vec![Type::VARCHAR];

        let and_application = match self.application.is_some() {
            true => {
                types.push(Type::VARCHAR);
                r#"
            AND
                APPLICATION = $2
"#
            }
            false => "",
        };

        // We retrieve the next thing that is due for being woken. We fetch only one, and push
        // its due time by a short interval past now. That way others would not see it right away.
        // The reconciliation will be processed and should update the waker too.
        //
        // NOTE: If we plan on scheduling the wakeup through something that buffers (like Kafka)
        // then it would not work to just delay for a short amount of time, as we don't know when
        // the wakeup would be executed. In this case, we would need to send out the event, and then
        // clear the waker.

        let stmt = format!(
            r#"
UPDATE
    things
SET
    WAKER = NOW() + $1::interval
WHERE
    UID = (
        SELECT
            UID
        FROM
            things
        WHERE
                WAKER <= NOW()
{and_application}

        ORDER BY
            WAKER ASC 
        
        LIMIT 1
        FOR UPDATE SKIP LOCKED

    )

RETURNING
    APPLICATION, NAME, UID, RESOURCE_VERSION, WAKER_REASONS
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
    // the amount of time to delay the waker by when processing
    waker_delay: &'r str,
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
        waker_delay: &'r str,
        f: &'r F,
    ) -> Self {
        Self {
            con,
            stmt,
            application,
            waker_delay,
            f,
        }
    }

    async fn run(self) -> anyhow::Result<()> {
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

    async fn tick_next(&self, stmt: &Statement) -> anyhow::Result<bool> {
        let row = match &self.application {
            Some(application) => {
                self.con
                    .query_opt(stmt, &[&self.waker_delay, application])
                    .await
            }
            None => self.con.query_opt(stmt, &[&self.waker_delay]).await,
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

                self.clear_waker(application, thing, uid, resource_version)
                    .await?;

                // done with this entry

                true
            }
            None => false,
        })
    }

    async fn clear_waker(
        &self,

        application: String,
        thing: String,
        uid: Uuid,
        resource_version: Uuid,
    ) -> anyhow::Result<()> {
        // we try to clear the waker. It might already be cleared due to the fact that we woke
        // that thing up. But it might also be that the wakeup event is still queued, so we don't
        // want to wake it up too often.

        let stmt = self
            .con
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

        let result = self
            .con
            .execute(&stmt, &[&application, &thing, &uid, &resource_version])
            .await?;

        if result == 0 {
            log::debug!("Lost oplock clearing waker. Don't worry.")
        }

        Ok(())
    }
}
