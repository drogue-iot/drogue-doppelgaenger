mod setup;

use crate::setup::Context;
use drogue_doppelgaenger_core::model::{Metadata, Thing};
use std::collections::BTreeMap;

#[tokio::test]
async fn basic() {
    let Context {
        service, notifier, ..
    } = setup::setup();

    service
        .create(Thing::new("default", "thing1"))
        .await
        .unwrap();
    let thing = service
        .get(&("default", "thing1").into())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(thing.metadata.application, "default");
    assert_eq!(thing.metadata.name, "thing1");

    assert_eq!(notifier.drain().await, vec![thing]);
}

#[tokio::test]
async fn delete() {
    let Context {
        service, notifier, ..
    } = setup::setup();

    service
        .create(Thing::new("default", "thing1"))
        .await
        .unwrap();

    let id = ("default", "thing1").into();

    let thing = service.get(&id).await.unwrap().unwrap();

    assert_eq!(thing.metadata.application, "default");
    assert_eq!(thing.metadata.name, "thing1");

    assert_eq!(notifier.drain().await, vec![thing]);

    let found = service.delete(&id).await.unwrap();
    assert_eq!(found, true);
    let found = service.delete(&id).await.unwrap();
    assert_eq!(found, false);
}

#[tokio::test]
async fn update() {
    let Context {
        service, notifier, ..
    } = setup::setup();

    service
        .create(Thing::new("default", "thing1"))
        .await
        .unwrap();

    let id = ("default", "thing1").into();

    let thing = service.get(&id).await.unwrap().unwrap();

    assert_eq!(thing.metadata.application, "default");
    assert_eq!(thing.metadata.name, "thing1");

    assert_eq!(notifier.drain().await, vec![thing.clone()]);

    let thing_1 = Thing {
        metadata: Metadata {
            // try change immutable fields
            application: "something".to_string(),
            name: "thing2".to_string(),
            annotations: {
                // make some change
                let mut annotations = BTreeMap::new();
                annotations.insert("foo".to_string(), "bar".to_string());
                annotations
            },
            ..thing.metadata.clone()
        },
        ..thing.clone()
    };

    let thing_1 = service.update(&id, thing_1).await.unwrap();

    assert_eq!(notifier.drain().await, vec![thing_1.clone()]);

    // immutable metadata must not change
    assert_eq!(thing_1.metadata.application, "default");
    assert_eq!(thing_1.metadata.name, "thing1");
    assert_eq!(thing_1.metadata.uid, thing.metadata.uid);
    assert_eq!(
        thing_1.metadata.creation_timestamp,
        thing.metadata.creation_timestamp
    );
    assert_eq!(
        thing_1.metadata.generation,
        thing.metadata.generation.map(|g| g + 1)
    );
    assert_ne!(
        thing_1.metadata.resource_version,
        thing.metadata.resource_version
    );

    // fetching again must return the same result

    let thing_1 = service
        .get(&id)
        .await
        .unwrap()
        .expect("Thing must be found");

    // immutable metadata must not change
    assert_eq!(thing_1.metadata.application, "default");
    assert_eq!(thing_1.metadata.name, "thing1");
    assert_eq!(thing_1.metadata.uid, thing.metadata.uid);
    assert_eq!(
        thing_1.metadata.creation_timestamp,
        thing.metadata.creation_timestamp
    );
    assert_eq!(
        thing_1.metadata.generation,
        thing.metadata.generation.map(|g| g + 1)
    );
    assert_ne!(
        thing_1.metadata.resource_version,
        thing.metadata.resource_version
    );
}

/// Testing the case that a change isn't a change.
#[tokio::test]
async fn update_no_change() {
    let Context {
        service, notifier, ..
    } = setup::setup();

    service
        .create(Thing::new("default", "thing1"))
        .await
        .unwrap();

    let id = ("default", "thing1").into();

    let thing = service.get(&id).await.unwrap().unwrap();

    assert_eq!(thing.metadata.application, "default");
    assert_eq!(thing.metadata.name, "thing1");

    assert_eq!(notifier.drain().await, vec![thing.clone()]);

    let thing_1 = service.update(&id, thing.clone()).await.unwrap();

    assert_eq!(notifier.drain().await, vec![]);

    assert_eq!(thing_1, thing);
}
