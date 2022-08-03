mod deno;
mod desired;
mod recon;

use crate::machine::recon::{Reconciler, ScriptAction};
use crate::{
    command::Command,
    machine::deno::{DenoOptions, Json},
    model::{Code, JsonSchema, Metadata, Schema, Thing, ThingState},
    processor::Message,
};
use anyhow::anyhow;
use deno_core::url::Url;
use jsonschema::{Draft, JSONSchema, SchemaResolver, SchemaResolverError};
use lazy_static::lazy_static;
use prometheus::{register_histogram, Histogram};
use serde_json::Value;
use std::{convert::Infallible, fmt::Debug, future::Future, sync::Arc};

lazy_static! {
    static ref TIMER_DELAY: Histogram =
        register_histogram!("timer_delay", "Amount of time by which timers are delayed").unwrap();
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OutboxMessage {
    pub thing: String,
    pub message: Message,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Mutator: {0}")]
    Mutator(Box<dyn std::error::Error + Send + Sync>),
    #[error("Reconciler: {0}")]
    Reconcile(#[source] anyhow::Error),
    #[error("Validation failed: {0}")]
    Validation(#[source] anyhow::Error),
    #[error("Internal: {0}")]
    Internal(#[source] anyhow::Error),
}

/// The state machine runner. Good for a single run.
pub struct Machine {
    thing: Thing,
}

pub struct Outcome {
    pub new_thing: Thing,
    pub outbox: Vec<OutboxMessage>,
    pub commands: Vec<Command>,
}

pub struct DeletionOutcome {
    pub thing: Thing,
    pub outbox: Vec<OutboxMessage>,
}

impl Machine {
    pub fn new(thing: Thing) -> Self {
        Self { thing }
    }

    /// Run actions for creating a new thing.
    pub async fn create(new_thing: Thing) -> Result<Outcome, Error> {
        // Creating means that we start with an empty thing, and then set the initial state.
        // This allows to run through the reconciliation initially.
        let outcome = Self::new(Thing::new(
            &new_thing.metadata.application,
            &new_thing.metadata.name,
        ))
        .update(|_| async { Ok::<_, Infallible>(new_thing) })
        .await?;

        // done
        Ok(outcome)
    }

    /// Run an update.
    pub async fn update<F, Fut, E>(self, f: F) -> Result<Outcome, Error>
    where
        F: FnOnce(Thing) -> Fut,
        Fut: Future<Output = Result<Thing, E>>,
        E: std::error::Error + Send + Sync + 'static,
    {
        // capture immutable or internal metadata
        let Metadata {
            name,
            application,
            uid,
            creation_timestamp,
            deletion_timestamp,
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

        let Outcome {
            new_thing,
            outbox,
            commands,
        } = Reconciler::new(original_thing, new_thing).run().await?;

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
                deletion_timestamp,
                generation,
                resource_version,
                ..new_thing.metadata
            },
            ..new_thing
        };

        // done

        Ok(Outcome {
            new_thing,
            outbox,
            commands,
        })
    }

    pub async fn delete(thing: Thing) -> Result<DeletionOutcome, Error> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);

        let thing = Arc::new(thing);
        let mut deletion_outbox = vec![];

        for (name, deleting) in &thing.reconciliation.deleting {
            match &deleting.code {
                Code::JavaScript(script) => {
                    // We align with the names the other scripts
                    #[derive(Clone, Debug, serde::Serialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Input {
                        current_state: Arc<Thing>,
                        new_state: Arc<Thing>,
                        action: ScriptAction,

                        outbox: Vec<OutboxMessage>,
                        logs: Vec<String>,
                    }
                    #[derive(Clone, Default, Debug, serde::Deserialize)]
                    #[serde(rename_all = "camelCase")]
                    struct Output {
                        #[serde(default)]
                        outbox: Vec<OutboxMessage>,
                        #[serde(default)]
                        logs: Vec<String>,
                    }

                    let exec = deno::Execution::new(
                        format!("delete-{}", name),
                        script,
                        DenoOptions { deadline },
                    )
                    .run::<_, Json<Output>, ()>(Input {
                        current_state: thing.clone(),
                        new_state: thing.clone(),
                        action: ScriptAction::Deleting,
                        outbox: vec![],
                        logs: vec![],
                    })
                    .await
                    .map_err(Error::Reconcile)?;

                    // we can only ignore logs, as either we succeed (and would delete the logs)
                    // or we fail, and don't store them.
                    let Output {
                        outbox,
                        logs: _logs,
                    } = exec.output.0;
                    deletion_outbox.extend(outbox)
                }
            }
        }

        Ok(DeletionOutcome {
            thing: (*thing).clone(),
            outbox: deletion_outbox,
        })
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

    #[tokio::test]
    async fn test_create() {
        let Outcome {
            new_thing,
            outbox,
            commands,
        } = Machine::create(test_thing()).await.unwrap();

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
                internal: None,
            },
            new_thing
        );

        assert_eq!(outbox, vec![]);
        assert_eq!(commands, vec![]);
    }

    #[tokio::test]
    async fn test_update_simple_1() {
        let last_update = Utc::now();
        let machine = Machine::new(test_thing());
        let Outcome {
            new_thing,
            outbox,
            commands,
        } = machine
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
        assert_eq!(commands, vec![]);
    }

    const UID: &str = "3952a802-01e8-11ed-a9c0-d45d6455d2cc";

    fn creation_timestamp() -> DateTime<Utc> {
        Utc.ymd(2022, 1, 1).and_hms(12, 42, 0)
    }

    fn test_metadata() -> Metadata {
        Metadata {
            name: "default".to_string(),
            application: "default".to_string(),
            uid: Some(UID.to_string()),
            creation_timestamp: Some(creation_timestamp()),
            deletion_timestamp: None,
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
}
