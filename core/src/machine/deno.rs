use super::Outgoing;
use crate::model::Thing;
use anyhow::bail;
use chrono::Duration;
use deno_core::{serde_v8, v8, Extension, JsRuntime, RuntimeOptions};
use serde::{Deserialize, Serialize};
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
const KEY_WAKER: &str = "waker";

pub struct ExecutionResult<O, T = Value> {
    pub output: O,
    pub value: T,
}

/// Execute a deno script
pub async fn execute<I, O, R>(
    name: &str,
    script: &str,
    input: I,
    opts: DenoOptions,
) -> anyhow::Result<ExecutionResult<O, R>>
where
    I: Serialize + Send + 'static,
    for<'de> O: Serialize + Deserialize<'de> + Default + Send + 'static,
    for<'de> R: Deserialize<'de> + Default + Send + 'static,
{
    let script = script.to_string();
    let name = name.to_string();

    Ok(Handle::current()
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

            set_context::<_, O>(&mut runtime, input)?;

            let global = runtime.execute_script(&name, &script)?;

            let value = {
                let scope = &mut runtime.handle_scope();
                let local = v8::Local::new(scope, global);
                serde_v8::from_v8(scope, local)?
            };

            Handle::current().block_on(async { runtime.run_event_loop(false).await })?;
            // FIXME: eval late result

            let output = extract_context(&mut runtime)?;

            Ok::<_, anyhow::Error>(ExecutionResult { output, value })
        })
        .await??)
}

#[deprecated = "Switch over to the execute method, once it is cleaned up a bit"]
/// Run a deno script
///
/// TODO: consider keeping the runtime for this run.
pub async fn run(
    name: String,
    script: &str,
    current_thing: Arc<Thing>,
    new_thing: &Thing,
    opts: DenoOptions,
) -> anyhow::Result<Outgoing> {
    let script = script.to_string();
    let new_thing = new_thing.clone();

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

            set_run_context(&mut runtime, &current_thing, &new_thing)?;

            // FIXME: take return value
            let _ = runtime.execute_script(&name, &script)?;

            Handle::current().block_on(async { runtime.run_event_loop(false).await })?;
            // FIXME: eval late result

            extract_run_context(&mut runtime)
        })
        .await??;

    Ok(thing)
}

fn set_context<I, O>(runtime: &mut JsRuntime, input: I) -> anyhow::Result<()>
where
    I: Serialize,
    O: Serialize + Default,
{
    let global = runtime.global_context();
    let scope = &mut runtime.handle_scope();
    let global = global.open(scope).global(scope);

    {
        let key = serde_v8::to_v8(scope, "input")?;
        let value = serde_v8::to_v8(scope, input)?;
        global.set(scope, key, value);
    }

    {
        let key = serde_v8::to_v8(scope, "output")?;
        let value = serde_v8::to_v8(scope, O::default())?;
        global.set(scope, key, value);
    }

    Ok(())
}

/// Extract the new state from the context
fn extract_context<O>(runtime: &mut JsRuntime) -> anyhow::Result<O>
where
    for<'de> O: Deserialize<'de> + Default,
{
    let global = runtime.global_context();
    let mut scope = &mut runtime.handle_scope();
    let global = global.open(scope).global(scope);

    let output = {
        let key = serde_v8::to_v8(&mut scope, "output")?;
        match global.get(scope, key) {
            Some(value) => serde_v8::from_v8(scope, value)?,
            None => O::default(),
        }
    };

    Ok(output)
}

/// Set the current and new state for the context
fn set_run_context(
    runtime: &mut JsRuntime,
    current_state: &Thing,
    new_state: &Thing,
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
fn extract_run_context(runtime: &mut JsRuntime) -> anyhow::Result<Outgoing> {
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

    let logs = {
        let key = serde_v8::to_v8(&mut scope, KEY_LOGS)?;
        match global.get(scope, key) {
            Some(value) => serde_v8::from_v8(scope, value)?,
            None => vec![],
        }
    };

    let waker = {
        let key = serde_v8::to_v8(&mut scope, KEY_WAKER)?;
        match global.get(scope, key) {
            Some(value) => to_duration(serde_v8::from_v8(scope, value)?)?,
            None => None,
        }
    };

    Ok(Outgoing {
        new_thing,
        outbox,
        logs,
        waker,
    })
}

/// convert a JavaScript value into a duration
fn to_duration(value: Value) -> anyhow::Result<Option<Duration>> {
    Ok(match value {
        Value::String(time) => {
            let duration = humantime::parse_duration(&time)?;
            Some(Duration::from_std(duration)?)
        }
        Value::Number(seconds) => {
            if let Some(seconds) = seconds.as_i64() {
                if seconds > 0 {
                    return Ok(Some(Duration::seconds(seconds)));
                }
            } else if let Some(_) = seconds.as_u64() {
                // we can be sure it doesn't fit into an i64
                return Ok(Some(Duration::seconds(i64::MAX)));
            } else if let Some(seconds) = seconds.as_f64() {
                if seconds > i64::MAX as f64 {
                    return Ok(Some(Duration::seconds(i64::MAX)));
                } else if seconds > 0f64 {
                    return Ok(Some(Duration::seconds(seconds as i64)));
                }
            }
            None
        }
        _ => None,
    })
}
