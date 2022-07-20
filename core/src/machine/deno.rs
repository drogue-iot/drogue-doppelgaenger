use super::Outgoing;
use crate::model::Thing;
use anyhow::bail;
use deno_core::{serde_v8, v8, Extension, JsRuntime, RuntimeOptions};
use serde_json::Value;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::time::Instant;

#[derive(Clone, Debug)]
pub struct DenoOptions {
    pub deadline: Instant,
}

const KEY_CURRENT_STATE: &str = "currentState";
const KEY_NEW_STATE: &str = "newState";
const KEY_OUTBOX: &str = "outbox";
const KEY_LOGS: &str = "logs";

/// Run a deno script
///
/// TODO: consider keeping the runtime for this run.
pub async fn run(
    name: String,
    script: String,
    current_thing: Arc<Thing>,
    new_thing: Thing,
    opts: DenoOptions,
) -> anyhow::Result<Outgoing> {
    let thing = Handle::current()
        .spawn_blocking(move || {
            // disable some operations
            let disable = Extension::builder()
                .middleware(|op| match op.name {
                    // the user won't see it, and it spams our logs
                    "op_print" => op.disable(),
                    _ => op,
                })
                .build();

            // FIXME: doesn't work as advertised, we keep it anyway
            let create_params = v8::Isolate::create_params().heap_limits(0, 3 * 1024 * 1024);

            let mut runtime = JsRuntime::new(RuntimeOptions {
                create_params: Some(create_params),
                extensions: vec![disable],
                ..Default::default()
            });

            let isolate = runtime.v8_isolate().thread_safe_handle();
            runtime.add_near_heap_limit_callback(move |_, current| {
                // FIXME: again, this currently doesn't work properly, we keep it anyway
                isolate.terminate_execution();
                current * 2
            });

            let isolate = runtime.v8_isolate().thread_safe_handle();
            Handle::current().spawn(async move {
                tokio::time::sleep_until(opts.deadline).await;
                isolate.terminate_execution();
            });

            set_context(&mut runtime, &current_thing, new_thing)?;

            // FIXME: take return value
            let _ = runtime.execute_script(&name, &script)?;

            Handle::current().block_on(async { runtime.run_event_loop(false).await })?;
            // FIXME: eval late result

            extract_context(&mut runtime)
        })
        .await??;

    Ok(thing)
}

/// Set the current and new state for the context
fn set_context(
    runtime: &mut JsRuntime,
    current_state: &Thing,
    new_state: Thing,
) -> anyhow::Result<()> {
    let global = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let global = global.open(scope).global(scope);

    {
        let key = serde_v8::to_v8(scope, KEY_CURRENT_STATE)?;
        let value = serde_v8::to_v8(scope, current_state)?;
        global.set(scope, key, value);
    }

    {
        let key = serde_v8::to_v8(scope, KEY_NEW_STATE)?;
        let value = serde_v8::to_v8(scope, new_state)?;
        global.set(scope, key, value);
    }

    {
        let key = serde_v8::to_v8(scope, KEY_OUTBOX)?;
        let value = serde_v8::to_v8(scope, Vec::<Value>::new())?;
        global.set(scope, key, value);
    }

    {
        let key = serde_v8::to_v8(scope, KEY_LOGS)?;
        let value = serde_v8::to_v8(scope, Vec::<Value>::new())?;
        global.set(scope, key, value);
    }

    Ok(())
}

/// Extract the new state from the context
fn extract_context(runtime: &mut JsRuntime) -> anyhow::Result<Outgoing> {
    let global = runtime.global_context();

    let mut scope = &mut runtime.handle_scope();

    let global = global.open(scope).global(scope);

    let new_thing = {
        let key = serde_v8::to_v8(&mut scope, KEY_NEW_STATE)?;
        match global.get(scope, key) {
            Some(value) => serde_v8::from_v8(scope, value)?,
            None => bail!("Script removed new state"),
        }
    };

    let outbox = {
        let key = serde_v8::to_v8(&mut scope, KEY_OUTBOX)?;
        match global.get(scope, key) {
            Some(value) => serde_v8::from_v8(scope, value)?,
            None => vec![],
        }
    };

    let log = {
        let key = serde_v8::to_v8(&mut scope, KEY_LOGS)?;
        match global.get(scope, key) {
            Some(value) => serde_v8::from_v8(scope, value)?,
            None => vec![],
        }
    };

    Ok(Outgoing {
        new_thing,
        outbox,
        log,
    })
}
