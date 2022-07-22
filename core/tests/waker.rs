mod setup;

use crate::setup::{setup, RunningContext};
use chrono::Utc;
use drogue_doppelgaenger_core::model::{Code, Reconciliation, Thing, WakerReason};
use drogue_doppelgaenger_core::service::Id;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

#[tokio::test]
async fn test_process() {
    let RunningContext {
        service,
        notifier,
        runner,
        ..
    } = setup().run(true);

    let id = Id::new("default", "thing1");
    let thing = service
        .create(Thing {
            reconciliation: Reconciliation {
                changed: {
                    let mut changed = BTreeMap::new();
                    changed.insert(
                        "change".to_string(),
                        Code::Script(
                            r#"
function wakeup(when) {
    waker = when;
}

if (newState.reportedState?.["foo"] === undefined) {
    if (newState.metadata.annotations?.["test"] === "true") {
        newState.reportedState = {};
        newState.reportedState["foo"] = {
            value: "bar",
            lastUpdate: new Date().toISOString(),
        }
    } else {
        newState.metadata.annotations = {"test": "true"};
        wakeup("5s");
    }
}

"#
                            .to_string(),
                        )
                        .into(),
                    );
                    changed
                },
            },
            ..Thing::with_id(&id)
        })
        .await
        .unwrap();

    let wakeup = thing.internal.unwrap().waker.unwrap();
    assert_eq!(wakeup.why, BTreeSet::from([WakerReason::Reconcile]));

    assert!(wakeup.when > Utc::now());

    // it should also have out annotation

    assert_eq!(
        thing.metadata.annotations.get("test").map(|s| s.as_str()),
        Some("true"),
    );

    // wait until the waker should have processed

    tokio::time::sleep_until(tokio::time::Instant::now() + Duration::from_secs(7)).await;

    // check again

    let thing = service.get(&id).await.unwrap().unwrap();

    assert_eq!(
        thing
            .internal
            .as_ref()
            .unwrap()
            .waker
            .as_ref()
            .unwrap()
            .why
            .clone()
            .into_iter()
            .collect::<Vec<_>>(),
        &[WakerReason::Reconcile]
    );

    assert_eq!(thing.reported_state.get("foo").unwrap().value, json!("bar"));

    // there are two events, one for the created and a second one for the timer
    assert_eq!(notifier.drain().await.len(), 2);

    // shutdown runner
    runner.shutdown().await.unwrap();
}
