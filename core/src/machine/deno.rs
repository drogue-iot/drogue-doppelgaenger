use deno_core::{include_js_files, serde_v8, v8, Extension, JsRuntime, RuntimeOptions};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use tokio::{runtime::Handle, task::JoinHandle, time::Instant};
use tracing::{instrument, Span};

#[derive(Clone, Debug)]
pub struct DenoOptions {
    pub deadline: Instant,
}

pub trait Injectable: Sized + Send {
    type Error: std::error::Error + Send + Sync;

    fn inject(self, runtime: &mut JsRuntime, name: &str) -> Result<(), Self::Error>;
}

pub trait Extractable: Sized + Send {
    type Error: std::error::Error + Send + Sync;

    fn extract(runtime: &mut JsRuntime, name: &str) -> Result<Self, Self::Error>;
}

pub trait Returnable: Sized + Send {
    type Error: std::error::Error + Send + Sync;

    fn r#return(
        runtime: &mut JsRuntime,
        global: v8::Global<v8::Value>,
    ) -> Result<Self, Self::Error>;
}

impl<T> Injectable for T
where
    T: Serialize + Send,
{
    type Error = serde_v8::Error;

    fn inject(self, runtime: &mut JsRuntime, name: &str) -> Result<(), Self::Error> {
        let global = runtime.global_context();
        let scope = &mut runtime.handle_scope();
        let global = global.open(scope).global(scope);

        let key = serde_v8::to_v8(scope, name)?;
        let value = serde_v8::to_v8(scope, self)?;
        global.set(scope, key, value);

        Ok(())
    }
}

impl<T> Injectable for Json<T>
where
    T: Serialize + Send,
{
    type Error = serde_v8::Error;

    fn inject(self, runtime: &mut JsRuntime, name: &str) -> Result<(), Self::Error> {
        self.0.inject(runtime, name)
    }
}

impl Extractable for () {
    type Error = Infallible;

    fn extract(_: &mut JsRuntime, _: &str) -> Result<Self, Self::Error> {
        Ok(())
    }
}

impl<T> Extractable for Option<T>
where
    for<'de> T: Deserialize<'de> + Send,
{
    type Error = serde_v8::Error;

    fn extract(runtime: &mut JsRuntime, name: &str) -> Result<Option<T>, Self::Error> {
        let global = runtime.global_context();
        let scope = &mut runtime.handle_scope();
        let global = global.open(scope).global(scope);

        let key = serde_v8::to_v8(scope, name)?;
        Ok(match global.get(scope, key) {
            Some(value) => Some(serde_v8::from_v8(scope, value)?),
            None => None,
        })
    }
}

impl<T> Extractable for Json<T>
where
    for<'de> T: Deserialize<'de> + Send + Default,
{
    type Error = serde_v8::Error;

    fn extract(runtime: &mut JsRuntime, name: &str) -> Result<Self, Self::Error> {
        Option::<T>::extract(runtime, name).map(|o| Json(o.unwrap_or_default()))
    }
}

pub struct Json<T>(pub T);

impl<T> Returnable for T
where
    for<'de> T: Deserialize<'de> + Send,
{
    type Error = serde_v8::Error;

    fn r#return(
        runtime: &mut JsRuntime,
        global: v8::Global<v8::Value>,
    ) -> Result<Self, Self::Error> {
        let scope = &mut runtime.handle_scope();
        let local = v8::Local::new(scope, global);
        serde_v8::from_v8(scope, local)
    }
}

pub struct Execution {
    name: String,
    code: String,
    opts: DenoOptions,
}

impl Execution {
    pub fn new<N, C>(name: N, code: C, opts: DenoOptions) -> Self
    where
        N: Into<String>,
        C: Into<String>,
    {
        Self {
            name: name.into(),
            code: code.into(),
            opts,
        }
    }

    #[instrument(skip_all)]
    fn create_runtime(&self) -> JsRuntime {
        // disable some operations
        let disable = Extension::builder()
            .middleware(|op| match op.name {
                // the user won't see it, and it spams our logs
                "op_print" => op.disable(),
                _ => op,
            })
            .build();

        let api = Extension::builder()
            .js(include_js_files!(prefix "drogue:extensions/api", "js/core.js",))
            .build();

        tracing::info!("Built extensions");

        // FIXME: doesn't work as advertised, we keep it anyway
        let create_params = v8::Isolate::create_params().heap_limits(0, 3 * 1024 * 1024);

        tracing::info!("Created parameters");

        let mut runtime = JsRuntime::new(RuntimeOptions {
            create_params: Some(create_params),
            extensions: vec![disable, api],
            ..Default::default()
        });

        tracing::info!("Created runtime");

        let isolate = runtime.v8_isolate().thread_safe_handle();
        runtime.add_near_heap_limit_callback(move |_, current| {
            // FIXME: again, this currently doesn't work properly, we keep it anyway
            isolate.terminate_execution();
            current * 2
        });

        runtime
    }

    #[instrument(parent = parent, skip_all)]
    fn run_inner<I, O, R>(self, parent: Span, input: I) -> anyhow::Result<ExecutionResult<O, R>>
    where
        I: Injectable + 'static,
        O: Extractable + 'static,
        R: Returnable + 'static,
    {
        struct Deadline(JoinHandle<()>);
        impl Drop for Deadline {
            fn drop(&mut self) {
                tracing::warn!("Aborting script");
                self.0.abort();
            }
        }

        let mut runtime = self.create_runtime();

        let name = self.name;
        let code = self.code;

        let isolate = runtime.v8_isolate().thread_safe_handle();
        let deadline = Deadline(Handle::current().spawn(async move {
            tokio::time::sleep_until(self.opts.deadline).await;
            isolate.terminate_execution();
        }));

        input.inject(&mut runtime, "context")?;

        tracing::info!("Execute script");
        let global = runtime.execute_script(&name, &code)?;

        let return_value = R::r#return(&mut runtime, global)?;

        Handle::current().block_on(async { runtime.run_event_loop(false).await })?;

        tracing::info!("Awaited event loop");

        // FIXME: eval late result

        // stop the deadline watcher
        drop(deadline);

        //let output = extract_context(&mut runtime)?;
        let output = O::extract(&mut runtime, "context")?;

        Ok::<_, anyhow::Error>(ExecutionResult {
            output,
            return_value,
        })
    }

    #[instrument(skip_all, err)]
    pub async fn run<I, O, R>(self, input: I) -> anyhow::Result<ExecutionResult<O, R>>
    where
        I: Injectable + 'static,
        O: Extractable + 'static,
        R: Returnable + 'static,
    {
        let span = Span::current();
        Handle::current()
            .spawn_blocking(move || self.run_inner(span, input))
            .await?
    }
}

pub struct ExecutionResult<O, T> {
    pub output: O,
    pub return_value: T,
}

pub mod duration {
    use chrono::Duration;
    use serde::de;
    use std::fmt;

    pub fn deserialize<'de, D>(d: D) -> Result<Option<Duration>, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        d.deserialize_option(OptionDurationVisitor)
    }

    struct DurationVisitor;

    impl<'de> de::Visitor<'de> for DurationVisitor {
        type Value = Duration;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a duration in seconds or a humantime duration")
        }

        fn visit_i64<E>(self, value: i64) -> Result<Duration, E>
        where
            E: de::Error,
        {
            Ok(Duration::seconds(value))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let value = value.clamp(0, i64::MAX as u64) as i64;
            Ok(Duration::seconds(value))
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let value = value.clamp(i64::MIN as f64, i64::MAX as f64) as i64;
            Ok(Duration::seconds(value))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            let duration = humantime::parse_duration(v).map_err(de::Error::custom)?;
            Duration::from_std(duration).map_err(de::Error::custom)
        }
    }

    struct OptionDurationVisitor;

    impl<'de> de::Visitor<'de> for OptionDurationVisitor {
        type Value = Option<Duration>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a duration in seconds, a humantime duration, or none")
        }

        /// Deserialize a timestamp in seconds since the epoch
        fn visit_none<E>(self) -> Result<Option<Duration>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        /// Deserialize a timestamp in seconds since the epoch
        fn visit_some<D>(self, d: D) -> Result<Option<Duration>, D::Error>
        where
            D: de::Deserializer<'de>,
        {
            d.deserialize_any(DurationVisitor).map(Some)
        }

        /// Deserialize a timestamp in seconds since the epoch
        fn visit_unit<E>(self) -> Result<Option<Duration>, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod test {
    use chrono::Duration;
    use serde_json::{json, Value};

    #[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
    struct Test {
        #[serde(default, with = "super::duration")]
        duration: Option<Duration>,
    }

    pub fn assert(expected: Option<Duration>, from: Value) {
        assert_eq!(
            Test { duration: expected },
            serde_json::from_value(json!({ "duration": from })).unwrap(),
        );
    }

    #[test]
    pub fn test_deser_duration_seconds() {
        assert(Some(Duration::seconds(1)), json!(1));
        assert(Some(Duration::hours(1)), json!(3600));
    }

    #[test]
    pub fn test_deser_duration_negative_seconds() {
        assert(Some(Duration::seconds(-1)), json!(-1));
        assert(Some(Duration::hours(-1)), json!(-3600));
    }

    #[test]
    pub fn test_deser_duration_fractional() {
        assert(Some(Duration::seconds(1)), json!(1.01));
        assert(Some(Duration::hours(1)), json!(3600.01));
    }

    #[test]
    pub fn test_deser_duration_humantime() {
        assert(Some(Duration::seconds(1)), json!("1s"));
        assert(Some(Duration::hours(1)), json!("1h"));
    }

    #[test]
    pub fn test_deser_duration_null() {
        assert(None, Value::Null);
    }

    #[test]
    pub fn test_deser_duration_none() {
        assert_eq!(
            Test { duration: None },
            serde_json::from_value(json!({})).unwrap(),
        );
    }
}
