use crate::{
    command::Command,
    machine::{
        deno::{self, DenoOptions, Json},
        desired::{CommandBuilder, Context, DesiredReconciler, FeatureContext},
        Error, ExecutionResult, OutboxMessage, Outcome, TIMER_DELAY,
    },
    model::{
        Changed, Code, DesiredFeatureMethod, DesiredFeatureReconciliation, DesiredMode,
        Reconciliation, SyntheticType, Thing, Timer, WakerExt, WakerReason,
    },
};
use anyhow::anyhow;
use chrono::{DateTime, Duration, Utc};
use indexmap::IndexMap;
use serde_json::Value;
use std::sync::Arc;
use tracing::instrument;

#[derive(Clone, Debug, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ScriptAction {
    Changed,
    Timer,
    Deleting,
    Synthetic,
    DesiredReconciliation,
}

pub struct Reconciler {
    deadline: tokio::time::Instant,
    current_thing: Arc<Thing>,
    new_thing: Thing,
    outbox: Vec<OutboxMessage>,
    commands: Vec<Command>,
}

impl Reconciler {
    pub fn new(current_thing: Arc<Thing>, new_thing: Thing) -> Self {
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(1);

        Self {
            current_thing,
            new_thing,
            deadline,
            outbox: Default::default(),
            commands: Default::default(),
        }
    }

    #[instrument(skip_all, err)]
    pub async fn run(mut self) -> Result<Outcome, Error> {
        // cleanup first
        self.cleanup();

        // synthetics
        self.generate_synthetics().await?;

        // run code
        let Reconciliation {
            changed,
            timers,
            deleting: _,
        } = self.new_thing.reconciliation.clone();
        // reconcile changed and timers, but not deleting, as we don't delete
        self.reconcile_changed(changed).await?;
        self.reconcile_timers(timers).await?;

        // reconcile desired state
        self.reconcile_desired_state().await?;

        // detect reported state changes
        self.sync_reported_state();

        Ok(Outcome {
            new_thing: self.new_thing,
            outbox: self.outbox,
            commands: self.commands,
        })
    }

    fn cleanup(&mut self) {
        // clear reconcile waker
        self.new_thing.clear_wakeup(WakerReason::Reconcile);

        // clear old logs first, otherwise logging of state will continuously grow
        // FIXME: remove when we only send a view of the state to the reconcile code
        for (_, v) in &mut self.new_thing.reconciliation.changed {
            v.last_log.clear();
        }
    }

    /// Synchronize the reported state changes with the previous state
    ///
    /// In case a value changed, the timestamp will be set to "now", otherwise the timestamp
    /// will be copied from the previous value.
    fn sync_reported_state(&mut self) {
        let now = Utc::now();

        for (k, next) in &mut self.new_thing.reported_state {
            if let Some(previous) = self.current_thing.reported_state.get(k) {
                if previous.value != next.value {
                    // set to now
                    next.last_update = now;
                } else {
                    // restore timestamp
                    next.last_update = previous.last_update;
                }
            }
        }
    }

    #[instrument(skip_all, err)]
    async fn generate_synthetics(&mut self) -> Result<(), Error> {
        let now = Utc::now();

        let new_state = Arc::new(self.new_thing.clone());

        for (name, mut syn) in &mut self.new_thing.synthetic_state {
            let value =
                Self::run_synthetic(name, &syn.r#type, new_state.clone(), self.deadline).await?;
            if syn.value != value {
                syn.value = value;
                syn.last_update = now;
            }
        }

        Ok(())
    }

    /// sync the state with the reported and expected state
    fn sync_desired_state(&mut self) -> Result<(), Error> {
        let mut waker = self.new_thing.waker();

        for (name, mut desired) in &mut self.new_thing.desired_state {
            // update the last change timestamp

            // find the current value
            let reported_value = self
                .new_thing
                .synthetic_state
                .get(name)
                .map(|state| &state.value)
                .or_else(|| {
                    self.new_thing
                        .reported_state
                        .get(name)
                        .map(|state| &state.value)
                })
                .unwrap_or(&Value::Null);
            let desired_value = &desired.value;

            // check if there is a change from the previous state
            if let Some(previous) = self.current_thing.desired_state.get(name) {
                if previous.value != desired.value || previous.valid_until != desired.valid_until {
                    // desired value changed, start reconciling again
                    desired.reconciliation =
                        DesiredFeatureReconciliation::Reconciling { last_attempt: None };
                    desired.last_update = Utc::now();
                }
            }

            if matches!(desired.method, DesiredFeatureMethod::Manual) {
                continue;
            }

            match (&desired.reconciliation, &desired.mode) {
                // Mode is disabled, and we already are ...
                (DesiredFeatureReconciliation::Disabled { .. }, DesiredMode::Disabled) => {
                    // ... do nothing
                }

                // Mode is disabled, but we are not...
                (_, DesiredMode::Disabled) => {
                    // ... mark disabled
                    desired.reconciliation =
                        DesiredFeatureReconciliation::Disabled { when: Utc::now() };
                }

                // Mode is not disabled, but we are are
                (DesiredFeatureReconciliation::Disabled { .. }, _) => {
                    if reported_value != desired_value {
                        // not the same
                        if desired.valid_until.map(|u| u > Utc::now()).unwrap_or(true) {
                            // the value is still valid, back to reconciling
                            desired.reconciliation =
                                DesiredFeatureReconciliation::Reconciling { last_attempt: None };
                        } else {
                            // the value is no longer valid
                            desired.reconciliation = DesiredFeatureReconciliation::Failed {
                                when: Utc::now(),
                                reason: Some(
                                    "Activated reconciliation with expired value".to_string(),
                                ),
                            };
                        }
                    } else {
                        // equals => means success
                        desired.reconciliation =
                            DesiredFeatureReconciliation::Succeeded { when: Utc::now() }
                    }
                }

                // Mode is "keep sync", and we succeeded
                (DesiredFeatureReconciliation::Succeeded { .. }, DesiredMode::Sync) => {
                    // if we should keep it in sync, check values and if the value is still valid
                    if reported_value != desired_value
                        && desired.valid_until.map(|u| u > Utc::now()).unwrap_or(true)
                    {
                        // if not, back to reconciling
                        desired.reconciliation =
                            DesiredFeatureReconciliation::Reconciling { last_attempt: None };

                        if let Some(valid_until) = desired.valid_until {
                            // and set waker
                            waker.wakeup_at(valid_until, WakerReason::Reconcile);
                        }
                    }
                }

                // succeeded and not (sync), or failed
                (DesiredFeatureReconciliation::Succeeded { .. }, _)
                | (DesiredFeatureReconciliation::Failed { .. }, _) => {
                    // we do nothing
                }

                // we are reconciling
                (DesiredFeatureReconciliation::Reconciling { .. }, _) => {
                    if reported_value == desired_value {
                        // value changed to expected value -> success
                        desired.reconciliation =
                            DesiredFeatureReconciliation::Succeeded { when: Utc::now() };
                    } else if let Some(valid_until) = desired.valid_until {
                        // value did not change to expected value, and expired -> failure
                        if valid_until < Utc::now() {
                            desired.reconciliation = DesiredFeatureReconciliation::Failed {
                                when: Utc::now(),
                                reason: None,
                            };
                        } else {
                            // otherwise, start waker
                            waker.wakeup_at(valid_until, WakerReason::Reconcile);
                        }
                    }
                    // else -> keep going
                }
            }
        }

        // set possible waker

        self.new_thing.set_waker(waker);

        // done

        Ok(())
    }

    #[instrument(skip_all, err)]
    async fn reconcile_desired_state(&mut self) -> Result<(), Error> {
        // sync first
        self.sync_desired_state()?;

        // get the current waker
        let mut waker = self.new_thing.waker();
        let new_thing = Arc::new(self.new_thing.clone());

        let mut commands = CommandBuilder::default();

        let mut context = Context {
            new_thing,
            deadline: self.deadline,
            waker: &mut waker,
            commands: &mut commands,
        };

        // process next
        for (name, desired) in &mut self.new_thing.desired_state {
            let value = desired.value.clone();

            match &mut desired.reconciliation {
                DesiredFeatureReconciliation::Disabled { .. }
                | DesiredFeatureReconciliation::Succeeded { .. }
                | DesiredFeatureReconciliation::Failed { .. } => {
                    // we do nothing
                }
                DesiredFeatureReconciliation::Reconciling { last_attempt } => {
                    match &desired.method {
                        DesiredFeatureMethod::Manual | DesiredFeatureMethod::External => {
                            // we do nothing
                        }

                        DesiredFeatureMethod::Command(command) => command
                            .reconcile(
                                &mut context,
                                FeatureContext {
                                    name,
                                    last_attempt,
                                    value,
                                },
                            )
                            .await
                            .map_err(|err| Error::Reconcile(anyhow!(err)))?,

                        DesiredFeatureMethod::Code(code) => code
                            .reconcile(
                                &mut context,
                                FeatureContext {
                                    name,
                                    last_attempt,
                                    value,
                                },
                            )
                            .await
                            .map_err(Error::Reconcile)?,
                    }
                }
            }
        }

        self.commands.extend(
            commands
                .into_commands(&self.new_thing.metadata.application)
                .map_err(|err| Error::Reconcile(anyhow!(err)))?,
        );

        // set waker, possibly changed
        self.new_thing.set_waker(waker);

        // done
        Ok(())
    }

    #[instrument(skip_all, err)]
    async fn reconcile_changed(&mut self, changed: IndexMap<String, Changed>) -> Result<(), Error> {
        for (name, mut changed) in changed {
            let ExecutionResult { logs } = self
                .run_code(
                    format!("changed-{}", name),
                    ScriptAction::Changed,
                    &changed.code,
                )
                .await?;

            changed.last_log = logs;
            self.new_thing.reconciliation.changed.insert(name, changed);
        }

        Ok(())
    }

    #[instrument(skip_all, err)]
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
                                        .unwrap_or_else(|_| Duration::max_value()),
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

                    self.run_code(format!("timer-{}", name), ScriptAction::Timer, &timer.code)
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

    #[instrument(skip_all, fields(name, action), err)]
    async fn run_code(
        &mut self,
        name: String,
        action: ScriptAction,
        code: &Code,
    ) -> Result<ExecutionResult, Error> {
        match code {
            Code::JavaScript(script) => {
                #[derive(serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Input {
                    current_state: Arc<Thing>,
                    new_state: Thing,
                    action: ScriptAction,
                    // the following items are scooped off by the output, but we need to initialize
                    // them to present, but empty values for the scripts.
                    outbox: Vec<Value>,
                    logs: Vec<Value>,
                }

                #[derive(Clone, Default, Debug, serde::Deserialize)]
                #[serde(rename_all = "camelCase")]
                pub struct Output {
                    #[serde(default)]
                    new_state: Option<Thing>,
                    #[serde(default)]
                    outbox: Vec<OutboxMessage>,
                    #[serde(default)]
                    logs: Vec<String>,
                    #[serde(default, with = "deno::duration")]
                    waker: Option<Duration>,
                }

                let opts = DenoOptions {
                    deadline: self.deadline,
                };
                let deno = deno::Execution::new(name, script, opts);
                let out = deno
                    .run::<_, Json<Output>, ()>(Input {
                        current_state: self.current_thing.clone(),
                        new_state: self.new_thing.clone(),
                        action,
                        outbox: vec![],
                        logs: vec![],
                    })
                    .await
                    .map_err(Error::Reconcile)?;

                // FIXME: record error (if any)

                let Output {
                    new_state,
                    waker,
                    outbox,
                    logs,
                } = out.output.0;

                let mut new_state = new_state.unwrap_or_else(|| self.new_thing.clone());

                // schedule the waker, in the new state
                if let Some(duration) = waker {
                    new_state.wakeup(duration, WakerReason::Reconcile);
                }
                // set the new state
                self.new_thing = new_state;
                // extend outbox
                self.outbox.extend(outbox);

                // done
                Ok(ExecutionResult { logs })
            }
        }
    }

    #[instrument(skip_all, fields(name), err)]
    async fn run_synthetic(
        name: &str,
        r#type: &SyntheticType,
        new_state: Arc<Thing>,
        deadline: tokio::time::Instant,
    ) -> Result<Value, Error> {
        match r#type {
            SyntheticType::JavaScript(script) => {
                #[derive(serde::Serialize)]
                #[serde(rename_all = "camelCase")]
                struct Input {
                    new_state: Arc<Thing>,
                    action: ScriptAction,
                }

                let opts = DenoOptions { deadline };
                let deno = deno::Execution::new(name, script, opts);
                let out = deno
                    .run::<_, (), Value>(Input {
                        new_state,
                        action: ScriptAction::Synthetic,
                    })
                    .await
                    .map_err(Error::Reconcile)?;

                Ok(out.return_value)
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

#[cfg(test)]
mod test {

    use super::*;
    use chrono::{TimeZone, Utc};
    use std::time::Duration;

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
