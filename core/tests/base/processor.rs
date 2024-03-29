use crate::common::mock::{setup, RunningContext};
use drogue_doppelgaenger_core::{
    processor::{Event, Message},
    service::{Id, Service},
};
use serde_json::json;

#[tokio::test]
async fn test_process() {
    let RunningContext {
        service,
        mut notifier,
        runner,
        ..
    } = setup().run(false);

    let id = Id::new("default", "thing1");
    let thing = service.create(id.make_thing()).await.unwrap();

    assert_eq!(notifier.drain().await, vec![thing.clone()]);

    log::debug!("Thing created, now update");

    // run a change

    runner
        .send_wait(Event::new(
            "default",
            "thing1",
            Message::report_state(false).state("foo", "bar"),
        ))
        .await
        .unwrap();

    // should see the change
    let thing = service.get(&id).await.unwrap().expect("Thing to be found");
    // and get notified
    assert_eq!(notifier.drain().await, vec![thing.clone()]);

    assert_eq!(thing.metadata.generation, Some(2));
    assert_eq!(thing.reported_state.len(), 1);
    assert_eq!(thing.reported_state.get("foo").unwrap().value, json!("bar"));

    // run another change, that doesn't change

    runner
        .send_wait(Event::new(
            "default",
            "thing1",
            Message::report_state(false).state("foo", "bar"),
        ))
        .await
        .unwrap();

    let thing = service.get(&id).await.unwrap().expect("Thing to be found");

    assert_eq!(notifier.drain().await, vec![]);

    assert_eq!(thing.metadata.generation, Some(2));
    assert_eq!(thing.reported_state.len(), 1);
    assert_eq!(thing.reported_state.get("foo").unwrap().value, json!("bar"));

    // shutdown runner
    runner.shutdown().await.unwrap();
}
