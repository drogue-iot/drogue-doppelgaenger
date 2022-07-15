mod deno;

use crate::machine::deno::DenoOptions;
use crate::model::{Changed, JsonSchema, Metadata, Schema, Thing, ThingState};
use anyhow::anyhow;
use deno_core::url::Url;
use jsonschema::{Draft, JSONSchema, SchemaResolver, SchemaResolverError};
use serde_json::Value;
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::sync::Arc;

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

pub struct Machine {
    thing: Thing,
}

impl Machine {
    pub fn new(thing: Thing) -> Self {
        Self { thing }
    }

    pub async fn create(new_thing: Thing) -> Result<Thing, Error> {
        // Creating means that we start with an empty thing, and then set the initial state.
        // This allows to run through the reconciliation initially.
        Self::new(Thing::new(
            &new_thing.metadata.application,
            &new_thing.metadata.name,
        ))
        .update(|_| async { Ok::<_, Infallible>(new_thing) })
        .await
    }

    pub async fn update<F, Fut, E>(self, f: F) -> Result<Thing, Error>
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

        let original_state = Arc::new(self.thing);

        // apply the update

        let new_state = f((*original_state).clone())
            .await
            .map_err(|err| Error::Mutator(Box::new(err)))?;

        // validate the outcome

        match &new_state.schema {
            Some(Schema::Json(schema)) => match schema {
                JsonSchema::Draft7(schema) => {
                    let compiled = JSONSchema::options()
                        .with_draft(Draft::Draft7)
                        .with_resolver(RejectResolver)
                        .compile(schema)
                        .map_err(|err| {
                            Error::Validation(anyhow!("Failed to compile schema: {err}"))
                        })?;

                    let state: ThingState = (&new_state).into();

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

        // reconcile the result

        let new_state = reconcile(original_state, new_state).await?;

        // reapply the captured metadata

        let new_state = Thing {
            metadata: Metadata {
                name,
                application,
                uid,
                creation_timestamp,
                generation,
                resource_version,
                ..new_state.metadata
            },
            ..new_state
        };

        // done

        Ok(new_state)
    }
}

async fn reconcile(current_thing: Arc<Thing>, mut new_thing: Thing) -> Result<Thing, Error> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(1);

    let reconciliations = new_thing.reconciliation.clone();

    for (name, changed) in reconciliations.changed {
        match changed {
            Changed::Script(script) => {
                new_thing = deno::run(
                    format!("reconcile-changed-{}", name),
                    script,
                    current_thing.clone(),
                    new_thing,
                    DenoOptions { deadline },
                )
                .await
                .map_err(Error::Reconcile)?;
            }
        }
    }

    Ok(new_thing)
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
        let thing = Machine::create(test_thing()).await.unwrap();

        assert_eq!(
            Thing {
                metadata: test_metadata(),
                schema: None,
                reported_state: Default::default(),
                desired_state: Default::default(),
                synthetic_state: Default::default(),
                reconciliation: Default::default(),
            },
            thing
        );
    }

    #[tokio::test]
    async fn test_update_simple_1() {
        let last_update = Utc::now();
        let machine = Machine::new(test_thing());
        let thing = machine
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
            },
            thing
        );
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
        }
    }
}
