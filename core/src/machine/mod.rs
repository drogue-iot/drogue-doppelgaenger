mod deno;

use crate::machine::deno::DenoOptions;
use crate::model::{
    Changed, Code, JsonSchema, Metadata, Reconciliation, Schema, SyntheticType, Thing, ThingState,
    Timer, WakerExt, WakerReason,
};
use crate::processor::Message;
use anyhow::anyhow;
use chrono::{DateTime, Duration, Utc};
use deno_core::url::Url;
use indexmap::IndexMap;
use jsonschema::{Draft, JSONSchema, SchemaResolver, SchemaResolverError};
use lazy_static::lazy_static;
use prometheus::{register_histogram, Histogram};
use serde_json::Value;
use std::{convert::Infallible, fmt::Debug, future::Future, sync::Arc};

lazy_static! {
    static ref TIMER_DELAY: Histogram =
        register_histogram!("timer_delay", "Amount of time by which timers are delayed").unwrap();
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Mutator: {0}")]
    Mutator(Box<dyn std::error::Error>),
    #[error("Reconciler: {0}")]
    Reconcile(#[source] anyhow::Error),
    #[error("Validation failed: {0}")]
    Validation(#[source] anyhow::Error),
    #[error("Internal: {0}")]
    Internal(#[source] anyhow::Error),
}

/// The state machine runner. Good for a single run of updates.
pub struct Machine {
    thing: Thing,
}

pub struct Outcome {
    pub new_thing: Thing,
    pub outbox: Vec<OutboxMessage>,
}

impl Machine {
    pub fn new(thing: Thing) -> Self {
        Self { thing }
    }

    pub async fn create(new_thing: Thing) -> Result<Outcome, Error> {
        // Creating means that we start with an empty thing, and then set the initial state.
        // This allows to run through the reconciliation initially.
        let outcome = Self::new(Thing::new(
            &new_thing.metadata.application,
            &new_thing.metadata.name,
        ))
        .update(|_| async { Ok::<_, Infallible>(new_thing) })
        .await?;

        // validate the outcome
        Self::validate(&outcome.new_thing)?;

        // done
        Ok(outcome)
    }

    pub async fn update<F, Fut, E>(self, f: F) -> Result<Outcome, Error>
    where
        F: FnOnce(Thing) -> Fut,
        Fut: Future<Output = Result<Thing, E>>,
        E: std::error::Error + 'static,
    {
        // capture immutable or internal metadata
        let Metadata {
            name,
            application,
            uid,
            creation_timestamp,
            generation,
            resource_version,
            annotations: _,
            labels: _,
        } = self.thing.metadata.clone();

        // start with original state

        let original_thing = Arc::new(self.thing);
        log::debug!("Original state: {original_thing:?}");

        // apply the update

        let new_thing = f((*original_thing).clone())
            .await
            .map_err(|err| Error::Mutator(Box::new(err)))?;

        log::debug!("New state (post-update: {new_thing:?}");

        // reconcile the result

        let Outcome { new_thing, outbox } =
            Reconciler::new(original_thing, new_thing).run().await?;

        log::debug!("New state (post-reconcile: {new_thing:?}");

        // validate the outcome
        Self::validate(&new_thing)?;

        log::debug!("New state (post-validate: {new_thing:?}");

        // reapply the captured metadata

        let new_thing = Thing {
            metadata: Metadata {
                name,
                application,
                uid,
                creation_timestamp,
                generation,
                resource_version,
                ..new_thing.metadata
            },
            ..new_thing
        };

        // done

        Ok(Outcome { new_thing, outbox })
    }

    fn validate(new_thing: &Thing) -> Result<(), Error> {
        match &new_thing.schema {
            Some(Schema::Json(schema)) => match schema {
                JsonSchema::Draft7(schema) => {
                    let compiled = JSONSchema::options()
                        .with_draft(Draft::Draft7)
                        .with_resolver(RejectResolver)
                        .compile(schema)
                        .map_err(|err| {
                            Error::Validation(anyhow!("Failed to compile schema: {err}"))
                        })?;

                    let state: ThingState = new_thing.into();

                    if !compiled.is_valid(&serde_json::to_value(&state).map_err(|err| {
                        Error::Internal(
                            anyhow::Error::from(err).context("Failed serializing thing state"),
                        )
                    })?) {
                        return Err(Error::Validation(anyhow!(
                            "New state did not validate against configured schema"
                        )));
                    }
                }
            },
            None => {}
        }

        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OutboxMessage {
    pub thing: String,
    pub message: Message,
}

#[derive(Clone, Debug)]
pub struct Outgoing {
    pub new_thing: Thing,
    pub outbox: Vec<OutboxMessage>,
    pub logs: Vec<String>,
    pub waker: Option<Duration>,
}

pub struct Reconciler {
    deadline: tokio::time::Instant,
    current_thing: Arc<Thing>,
    new_thing: Thing,
    outbox: Vec<OutboxMessage>,
}

impl Reconciler {
    pub fn new(current_thing: Arc<Thing>, new_thing: Thing) -> Self {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(1);

        Self {
            current_thing,
            new_thing,
            deadline,
            outbox: Default::default(),
        }
    }

    pub async fn run(mut self) -> Result<Outcome, Error> {
        // clear reconcile waker
        self.new_thing.clear_wakeup(WakerReason::Reconcile);

        // clear old logs first, otherwise logging of state will continuously grow
        // FIXME: remove when we only send a view of the state to the reconcile code
        for (_, v) in &mut self.new_thing.reconciliation.changed {
            v.last_log.clear();
        }

        // synthetics
        self.generate_synthetics().await?;

        // run code

        let Reconciliation { changed, timers } = self.new_thing.reconciliation.clone();

        self.reconcile_changed(changed).await?;
        self.reconcile_timers(timers).await?;

        Ok(Outcome {
            new_thing: self.new_thing,
            outbox: self.outbox,
        })
    }

    async fn generate_synthetics(&mut self) -> Result<(), Error> {
        let now = Utc::now();

        let new_state = self.new_thing.clone();

        for (name, mut syn) in &mut self.new_thing.synthetic_state {
            let value =
                Self::run_synthetic(&name, &syn.r#type, new_state.clone(), self.deadline).await?;
            if syn.value != value {
                syn.value = value;
                syn.last_update = now;
            }
        }

        Ok(())
    }

    async fn reconcile_changed(&mut self, changed: IndexMap<String, Changed>) -> Result<(), Error> {
        for (name, mut changed) in changed {
            let ExecutionResult { logs } = self
                .run_code(format!("changed-{}", name), &changed.code)
                .await?;

            changed.last_log = logs;
            self.new_thing.reconciliation.changed.insert(name, changed);
        }

        Ok(())
    }

    async fn reconcile_timers(&mut self, timers: IndexMap<String, Timer>) -> Result<(), Error> {
        for (name, mut timer) in timers {
            let due = match timer.stopped {
                true => {
                    // timer is stopped, just keep it stopped
                    timer.last_started = None;
                    None
                }
                false => {
                    let last_started = match timer.last_started {
                        None => {
                            let now = Utc::now();
                            timer.last_started = Some(now);
                            now
                        }
                        Some(last_started) => last_started,
                    };

                    // now check if the timer is due
                    match (timer.last_run, timer.initial_delay) {
                        (Some(last_run), _) => {
                            // timer already ran, check if it is due again
                            Some(Self::find_next_run_from(
                                last_started,
                                timer.period,
                                last_run,
                            ))
                        }
                        (None, None) => {
                            // timer never ran, and there is no delay, run now
                            Some(Utc::now())
                        }
                        (None, Some(initial_delay)) => {
                            // timer never ran, check it the first run is due
                            Some(
                                last_started
                                    + Duration::from_std(initial_delay)
                                        .unwrap_or(Duration::max_value()),
                            )
                        }
                    }
                }
            };

            if let Some(due) = due {
                let diff = Utc::now() - due;

                let next_run = if diff >= Duration::zero() {
                    log::debug!("Late by: {diff}");
                    TIMER_DELAY.observe(diff.num_milliseconds() as f64);

                    let now = Utc::now();

                    self.run_code(format!("timer-{}", name), &timer.code)
                        .await?;

                    let next_run =
                        Self::find_next_run(timer.last_started.unwrap_or(now), timer.period);

                    log::info!("Next run: {next_run}");

                    timer.last_run = Some(now);

                    next_run
                } else {
                    due
                };

                self.new_thing.wakeup_at(next_run, WakerReason::Reconcile);
            }

            self.new_thing.reconciliation.timers.insert(name, timer);
        }

        Ok(())
    }

    async fn run_code(&mut self, name: String, code: &Code) -> Result<ExecutionResult, Error> {
        match code {
            Code::JavaScript(script) => {
                let outgoing = deno::run(
                    name,
                    &script,
                    self.current_thing.clone(),
                    &self.new_thing,
                    DenoOptions {
                        deadline: self.deadline,
                    },
                )
                .await
                .map_err(Error::Reconcile)?;

                // FIXME: record error (if any)

                let Outgoing {
                    mut new_thing,
                    waker,
                    outbox,
                    logs,
                } = outgoing;

                // schedule the waker, in the new state
                if let Some(duration) = waker {
                    new_thing.wakeup(duration, WakerReason::Reconcile);
                }
                // set the new state
                self.new_thing = new_thing;
                // extend outbox
                self.outbox.extend(outbox);

                Ok(ExecutionResult { logs })
            }
        }
    }

    async fn run_synthetic(
        name: &str,
        r#type: &SyntheticType,
        new_state: Thing,
        deadline: tokio::time::Instant,
    ) -> Result<Value, Error> {
        match r#type {
            SyntheticType::JavaScript(script) => {
                #[derive(serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Input {
                    new_state: Thing,
                }

                #[derive(Default, serde::Serialize, serde::Deserialize)]
                struct Output {}

                let out: deno::ExecutionResult<Output> =
                    deno::execute(name, script, Input { new_state }, DenoOptions { deadline })
                        .await
                        .map_err(Error::Reconcile)?;

                Ok(out.value)
            }
            SyntheticType::Alias(alias) => match new_state.reported_state.get(alias) {
                Some(value) => Ok(value.value.clone()),
                None => Ok(Value::Null),
            },
        }
    }

    fn find_next_run_from(
        last_started: DateTime<Utc>,
        period: std::time::Duration,
        now: DateTime<Utc>,
    ) -> DateTime<Utc> {
        let period_ms = period.as_millis().clamp(0, u32::MAX as u128) as u32;
        let diff = (now - last_started).num_milliseconds();

        if diff < 0 {
            return Utc::now();
        }

        let diff = diff.clamp(0, u32::MAX as i64) as u32;
        let periods = (diff / period_ms) + 1;

        last_started + Duration::milliseconds((periods * period_ms) as i64)
    }

    fn find_next_run(last_started: DateTime<Utc>, period: std::time::Duration) -> DateTime<Utc> {
        Self::find_next_run_from(last_started, period, Utc::now())
    }
}

pub struct ExecutionResult {
    pub logs: Vec<String>,
}

pub struct RejectResolver;

impl SchemaResolver for RejectResolver {
    fn resolve(&self, _: &Value, _: &Url, _: &str) -> Result<Arc<Value>, SchemaResolverError> {
        Err(anyhow!("Schema resolving is not allowed."))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::model::{Metadata, ReportedFeature};
    use chrono::{DateTime, TimeZone, Utc};
    use std::collections::BTreeMap;
    use std::time::Duration;

    #[tokio::test]
    async fn test_create() {
        let Outcome { new_thing, outbox } = Machine::create(test_thing()).await.unwrap();

        // When creating, the machine will start with an empty thing, which doesn't have any
        // resource information. The machine will also ensure that this internal metadata is not
        // overridden externally. So the outcome must be, that the information is missing.
        let metadata = Metadata {
            uid: None,
            creation_timestamp: None,
            resource_version: None,
            generation: None,
            ..test_metadata()
        };

        assert_eq!(
            Thing {
                metadata,
                schema: None,
                reported_state: Default::default(),
                desired_state: Default::default(),
                synthetic_state: Default::default(),
                reconciliation: Default::default(),
                internal: None
            },
            new_thing
        );

        assert_eq!(outbox, vec![]);
    }

    #[tokio::test]
    async fn test_update_simple_1() {
        let last_update = Utc::now();
        let machine = Machine::new(test_thing());
        let Outcome { new_thing, outbox } = machine
            .update(|mut thing| async {
                thing.reported_state.insert(
                    "temperature".to_string(),
                    ReportedFeature {
                        last_update,
                        value: Default::default(),
                    },
                );
                Ok::<_, Infallible>(thing)
            })
            .await
            .unwrap();

        assert_eq!(
            Thing {
                metadata: test_metadata(),
                schema: None,
                reported_state: {
                    let mut r = BTreeMap::new();
                    r.insert(
                        "temperature".to_string(),
                        ReportedFeature {
                            last_update,
                            value: Default::default(),
                        },
                    );
                    r
                },
                desired_state: Default::default(),
                synthetic_state: Default::default(),
                reconciliation: Default::default(),
                internal: None
            },
            new_thing
        );

        assert_eq!(outbox, vec![]);
    }

    const UID: &str = "3952a802-01e8-11ed-a9c0-d45d6455d2cc";

    fn creation_timestamp() -> DateTime<Utc> {
        Utc.ymd(2022, 01, 01).and_hms(12, 42, 00)
    }

    fn test_metadata() -> Metadata {
        Metadata {
            name: "default".to_string(),
            application: "default".to_string(),
            uid: Some(UID.to_string()),
            creation_timestamp: Some(creation_timestamp()),
            generation: Some(1),
            resource_version: Some("1".to_string()),
            annotations: Default::default(),
            labels: Default::default(),
        }
    }

    fn test_thing() -> Thing {
        Thing {
            metadata: test_metadata(),
            schema: None,
            reported_state: Default::default(),
            desired_state: Default::default(),
            synthetic_state: Default::default(),
            reconciliation: Default::default(),
            internal: Default::default(),
        }
    }

    #[test]
    fn test_next() {
        assert_next((0, 0, 0), (0, 1, 0), 1, (0, 1, 1));
        assert_next((0, 0, 0), (0, 1, 2), 10, (0, 1, 10));
        assert_next((0, 0, 0), (0, 0, 1), 1, (0, 0, 2));
    }

    fn assert_next(
        started: (u32, u32, u32),
        now: (u32, u32, u32),
        period: u64,
        expected: (u32, u32, u32),
    ) {
        let day = Utc.ymd(2022, 1, 1);

        assert_eq!(
            Reconciler::find_next_run_from(
                day.and_hms(started.0, started.1, started.2),
                Duration::from_secs(period),
                day.and_hms(now.0, now.1, now.2)
            ),
            day.and_hms(expected.0, expected.1, expected.2)
        );
    }
}
