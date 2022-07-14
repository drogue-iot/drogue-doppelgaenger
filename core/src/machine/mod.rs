mod deno;

use crate::machine::deno::DenoOptions;
use crate::model::{Changed, Metadata, Thing};
use std::convert::Infallible;
use std::future::Future;
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Mutator: {0}")]
    Mutator(#[source] Box<dyn std::error::Error>),
    #[error("Reconciler: {0}")]
    Reconcile(#[source] anyhow::Error),
}

pub struct Machine {
    thing: Thing,
}

impl Machine {
    pub fn new(thing: Thing) -> Self {
        Self { thing }
    }

    pub async fn create(new_thing: Thing) -> Result<Thing, Error> {
        Self::new(Thing {
            metadata: new_thing.metadata.clone(),

            reported_state: Default::default(),
            desired_state: Default::default(),
            synthetic_state: Default::default(),

            reconciliation: Default::default(),
        })
        .update(|mut thing| async {
            thing.reported_state = new_thing.reported_state;
            thing.desired_state = new_thing.desired_state;
            thing.synthetic_state = new_thing.synthetic_state;
            Ok::<_, Infallible>(thing)
        })
        .await
    }

    pub async fn update<F, Fut, E>(self, f: F) -> Result<Thing, Error>
    where
        F: FnOnce(Thing) -> Fut,
        Fut: Future<Output = Result<Thing, E>>,
        E: std::error::Error + 'static,
    {
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

        let original_state = self.thing;
        let new_state = f(original_state.clone())
            .await
            .map_err(|err| Error::Mutator(Box::new(err)))?;

        let original_state = Arc::new(original_state);
        let new_state = reconcile(original_state, new_state).await?;

        // replace internal metadata
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
            reported_state: Default::default(),
            desired_state: Default::default(),
            synthetic_state: Default::default(),
            reconciliation: Default::default(),
        }
    }
}
