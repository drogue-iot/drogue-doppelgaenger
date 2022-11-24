use crate::common::mock::{setup, RunningContext};
use chrono::Utc;
use drogue_doppelgaenger_core::{
    model::WakerReason,
    service::{Id, Service},
};
use drogue_doppelgaenger_model::{Code, Reconciliation, Thing, Timer};
use indexmap::IndexMap;
use serde_json::json;
use std::{collections::BTreeSet, time::Duration};

#[tokio::test]
async fn test_process() {
    let RunningContext {
        service,
        mut notifier,
        runner,
        ..
    } = setup().run(true);

    let id = Id::new("default", "test_process");
    let thing = service
        .create(Thing {
            reconciliation: Reconciliation {
                changed: {
                    let mut changed = IndexMap::new();
                    changed.insert(
                        "change".to_string(),
                        Code::JavaScript(
                            r#"
function wakeup(when) {
    context.waker = when;
}

if (context.newState.reportedState?.["foo"] === undefined) {
    if (context.newState.metadata.annotations?.["test"] === "true") {
        context.newState.reportedState = {};
        context.newState.reportedState["foo"] = {
            value: "bar",
            lastUpdate: new Date().toISOString(),
        }
    } else {
        context.newState.metadata.annotations = {"test": "true"};
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
                ..Default::default()
            },
            ..id.make_thing()
        })
        .await
        .unwrap();

    let wakeup = thing.internal.unwrap().waker;
    assert_eq!(wakeup.why, BTreeSet::from([WakerReason::Reconcile]));

    assert!(wakeup.when.unwrap() > Utc::now());

    // it should also have our annotation

    assert_eq!(
        thing.metadata.annotations.get("test").map(|s| s.as_str()),
        Some("true"),
    );

    // wait until the waker should have processed

    tokio::time::sleep_until(tokio::time::Instant::now() + Duration::from_secs(7)).await;

    // check again

    let thing = service.get(&id).await.unwrap().unwrap();

    // waker expired, so it must be gone
    let internal = thing.internal.clone().unwrap_or_default();
    assert!(internal.waker.when.is_none());
    assert!(internal.waker.why.is_empty());

    assert_eq!(thing.reported_state.get("foo").unwrap().value, json!("bar"));

    // there are two events, one for the created and a second one for the timer
    assert_eq!(notifier.drain().await.len(), 2);

    // shutdown runner
    runner.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_timer() {
    let RunningContext {
        service,
        mut notifier,
        runner,
        ..
    } = setup().run(true);

    let id = Id::new("default", "test_timer");

    // Create a thing with a 5 second timer

    let thing = service
        .create(Thing {
            reconciliation: Reconciliation {
                timers: {
                    let mut timer = IndexMap::new();
                    timer.insert(
                        "timer1".to_string(),
                        Timer::new(
                            Duration::from_secs(5),
                            Some(Duration::from_secs(3)),
                            Code::JavaScript(
                                r#"
if (context.newState.metadata.annotations === undefined) {
    context.newState.metadata.annotations = {};
}
if (context.newState.reportedState === undefined) {
    context.newState.reportedState = {};
}

const lastUpdate = new Date().toISOString();
const value = (context.newState.reportedState["timer"]?.value || 0) + 1;
context.newState.reportedState["timer"] = { value, lastUpdate };
"#
                                .to_string(),
                            ),
                        ),
                    );
                    timer
                },
                ..Default::default()
            },
            ..id.make_thing()
        })
        .await
        .unwrap();

    // check that the timer didn't run yet
    let t = &thing.reconciliation.timers["timer1"];
    assert!(t.last_run.is_none());
    assert!(!t.stopped);
    assert!(t.last_started.is_some());

    // so the state must still be missing
    assert_eq!(thing.reported_state.get("timer").map(|f| &f.value), None);

    // check the waker
    let wakeup = thing.internal.unwrap().waker;
    assert_eq!(wakeup.why, BTreeSet::from([WakerReason::Reconcile]));
    assert!(wakeup.when.unwrap() > Utc::now());

    // wait until the initial delay has expired
    tokio::time::sleep_until(tokio::time::Instant::now() + Duration::from_secs(3 + 1)).await;

    // check again

    let thing = service.get(&id).await.unwrap().unwrap();

    let wakeup = thing.internal.unwrap().waker;
    assert_eq!(
        wakeup.why.clone().into_iter().collect::<Vec<_>>(),
        &[WakerReason::Reconcile]
    );
    let timer = &thing.reconciliation.timers["timer1"];
    assert!(timer.last_run.is_some());

    assert_eq!(
        thing.reported_state.get("timer").map(|f| &f.value),
        Some(&json!(1))
    );

    // wait until the timer expired again

    tokio::time::sleep_until(tokio::time::Instant::now() + Duration::from_secs(5 + 1)).await;

    let thing = service.get(&id).await.unwrap().unwrap();

    assert_eq!(
        thing.reported_state.get("timer").map(|f| &f.value),
        Some(&json!(2))
    );

    // events: creation, initial expiration, first true expiration
    assert_eq!(notifier.drain().await.len(), 3);

    // shutdown runner
    runner.shutdown().await.unwrap();
}
