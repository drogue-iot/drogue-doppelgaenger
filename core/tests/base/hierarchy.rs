use crate::common::mock::{setup, RunningContext};
use drogue_doppelgaenger_core::{
    model::{Code, Deleting, Thing},
    processor::{Event, Message},
    service::{AnnotationsUpdater, Id, UpdateOptions},
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn test_hierarchy() -> anyhow::Result<()> {
    let RunningContext {
        service,
        mut notifier,
        mut sink,
        runner,
        ..
    } = setup().run(true);

    let common = r#"
if (context.newState.metadata.annotations === undefined) {
    context.newState.metadata.annotations = {};
}

if (context.newState.reportedState === undefined) {
    context.newState.reportedState = {};
}

function $ref() {
    return context.newState.metadata.name;
}

function normalize(group) {
    if (group === undefined) {
        return group;
    }
    return group.split('/').filter(t => t !== "")
}

function parentGroup(group) {
    if (group === undefined) {
        return group;
    }
    if (group.length > 0) {
        return group.slice(0,-1);
    } else {
        return undefined;
    }
}

function registerChild(reg, thing, $ref) {
    if (reg) {
        const deleting = context.newState.reconciliation.deleting["hierarchy"];
        const changed = context.newState.reconciliation.changed["hierarchy"];
        sendMessage(thing, {registerChild: {$ref, template: {
            reconciliation: {
                changed: {
                    hierarchy: changed,
                },
                deleting: {
                    hierarchy: deleting,
                }
            }
        }}});
    } else {
        sendMessage(thing, {unregisterChild: {$ref}});
    }
}

function registerChannel(reg, device, channel) {
    log(`Register channel: ${device} / ${channel} (${reg})`);

    if (reg) {
        if (context.newState.metadata.annotations["io.drogue/device"] !== device
            || context.newState.metadata.annotations["io.drogue/channel"] !== channel 
            ) {
            context.newState.metadata.annotations["io.drogue/device"] = device;
            context.newState.metadata.annotations["io.drogue/channel"] = channel;
            registerChild(true, device, $ref())
        }
        
        context.newState.reportedState["$parent"] = {lastUpdate: new Date().toISOString(), value: device};
    } else {
        registerChild(false, device, $ref())
    }
}

function registerDevice(reg, device) {
    log(`Register device: ${device} (${reg})`);

    let group = normalize(context.newState.metadata.annotations["io.drogue/group"]);

    if (group !== undefined) {
        group = group.join('/');
        const parentStr = "/" + group;
        if (reg) {
            context.newState.metadata.annotations["io.drogue/device"] = device;
            if (context.currentState.metadata.annotations?.["io.drogue/group"] !== group) {
                registerChild(true, parentStr, $ref());
            }
            context.newState.reportedState["$parent"] = {lastUpdate: new Date().toISOString(), value: parentStr};
        } else {
            registerChild(false, parentStr, $ref());
        }
    }
}

function registerGroup(reg, group) {
    log(`Register group: ${group} (${reg})`);

    group = normalize(group);
    groupValue = group.join('/');
    const parent = parentGroup(group);

    if (parent !== undefined) {
        const parentStr = "/" + parent.join('/');
        if (reg) {
            if (context.newState.metadata.annotations["io.drogue/group"] !== groupValue) {
                context.newState.metadata.annotations["io.drogue/group"] = groupValue;
                
                registerChild(true, parentStr, $ref())
            }
            context.newState.reportedState["$parent"] = {lastUpdate: new Date().toISOString(), value: parentStr};
        } else {
            registerChild(false, parentStr, $ref())
        }
    }
}

function register(reg) {
    const ref = $ref();
    if (ref.startsWith('/')) {
        // group
        registerGroup(reg, ref);
    } else {
        const s = ref.split('/', 2);
        if (s.length >= 2) {
            // channel
            registerChannel(reg, s[0], s[1]);
        } else {
            // device
            registerDevice(reg, s[0]);
        }
    }
}

switch (context.action) {
    case "changed": {
        register(true);
        break;
    }
    case "deleting": {
        register(false);
        break;
    }
}
"#;

    // expect a group of [foo, bar, baz], a thing named "device" and a thing named "device/channel"
    let id = Id::new("default", "device/channel");
    let mut thing = Thing::with_id(&id);
    thing.reconciliation.deleting.insert(
        "hierarchy".to_string(),
        Deleting {
            code: Code::JavaScript(common.to_string()),
        },
    );
    thing.reconciliation.changed.insert(
        "hierarchy".to_string(),
        Code::JavaScript(common.to_string()).into(),
    );
    let thing = service.create(thing).await.unwrap();

    log::info!("Thing: {thing:#?}");

    assert_eq!(thing.metadata.annotations["io.drogue/device"], "device");
    assert_eq!(thing.metadata.annotations["io.drogue/channel"], "channel");

    // FIXME: should listen to events instead
    tokio::time::sleep(Duration::from_secs(1)).await;

    let device_id = Id::new("default", "device");
    service.get(&device_id).await?.expect("Device level thing");

    // now set the group
    let device = service
        .update(
            &device_id,
            &AnnotationsUpdater::new("io.drogue/group", "foo/bar/baz"),
            &UpdateOptions {
                ignore_unclean_inbox: false,
            },
        )
        .await?;

    log::info!("Device thing: {device:#?}");

    assert_eq!(
        device.metadata.annotations["io.drogue/group"],
        "foo/bar/baz"
    );

    // FIXME: should listen to events instead
    tokio::time::sleep(Duration::from_secs(1)).await;

    let group_l_3 = service
        .get(&Id::new("default", "/foo/bar/baz"))
        .await?
        .expect("Group level 3 thing");
    assert_eq!(
        group_l_3.metadata.annotations["io.drogue/group"],
        "foo/bar/baz"
    );
    assert_eq!(group_l_3.reported_state["$parent"].value, json!("/foo/bar"));

    let group_l_2 = service
        .get(&Id::new("default", "/foo/bar"))
        .await
        .unwrap()
        .expect("Group level 2 thing");
    assert_eq!(group_l_2.metadata.annotations["io.drogue/group"], "foo/bar");
    assert_eq!(group_l_2.reported_state["$parent"].value, json!("/foo"));

    let group_l_1 = service
        .get(&Id::new("default", "/foo"))
        .await
        .unwrap()
        .expect("Group level 1 thing");
    assert_eq!(group_l_1.metadata.annotations["io.drogue/group"], "foo");
    assert_eq!(group_l_1.reported_state["$parent"].value, json!("/"));

    let root = service
        .get(&Id::new("default", "/"))
        .await
        .unwrap()
        .expect("Root level thing");
    assert!(root.reported_state.get("$parent").is_none());

    // do some update, so ensure e.g. reported state updates don't cause any further events
    runner
        .send_wait(Event::new(
            &id.application,
            &id.thing,
            Message::report_state(true).state("foo", "bar"),
        ))
        .await
        .unwrap();

    // not start the destruction

    service
        .delete(&id, None)
        .await
        .expect("Deletion successful");

    // FIXME: should listen to events instead
    tokio::time::sleep(Duration::from_secs(2)).await;

    // everything must be gone
    for i in [
        "/",
        "/foo",
        "/foo/bar",
        "/foo/bar/baz",
        "device",
        "device/channel",
    ] {
        let thing = service.get(&Id::new("default", i)).await?;
        assert!(thing.is_none(), "Thing {i} must be gone: {thing:#?}",);
    }

    let changes = notifier.drain().await;
    for (n, c) in changes.iter().enumerate() {
        log::debug!("Change {n:2}: {c:?}")
    }
    assert_eq!(
        changes.len(),
        1 // create channel
        + 1 // auto-create device
        + 1 // manual update device
        + 4 // auto-create groups
        + 1 // reported state update
        + 1 // delete channel
        + 5 // auto-delete device and groups
    );

    let events = sink.drain().await;
    for (n, e) in events.iter().enumerate() {
        log::debug!("Event {n}: {e:?}")
    }
    assert_eq!(
        events.len(),
        1 // creating the device thing
        + 4 // creating the group things
        + 4 // deleting the group things
        + 1 // deleting the device thing
    );

    // done
    runner.shutdown().await.unwrap();
    Ok(())
}
