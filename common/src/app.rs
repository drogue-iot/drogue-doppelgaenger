// FIXME: ripped out of Drogue Cloud, need to have a common base

use futures::future::LocalBoxFuture;
use futures::stream::FuturesUnordered;

#[cfg(feature = "jaeger")]
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[macro_export]
macro_rules! app {
    () => {
        $crate::main!(run(Config::from_env()?).await)
    };
}

#[macro_export]
macro_rules! main {
    ($run:expr) => {{

        use std::io::Write;

        const VERSION: &str = env!("CARGO_PKG_VERSION");
        const NAME: &str = env!("CARGO_PKG_NAME");
        const DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

        use drogue_doppelgaenger_common::config::ConfigFromEnv;
        $crate::app::init();

        println!(r#"______ ______  _____  _____  _   _  _____   _____         _____ 
|  _  \| ___ \|  _  ||  __ \| | | ||  ___| |_   _|       |_   _|
| | | || |_/ /| | | || |  \/| | | || |__     | |    ___    | |  
| | | ||    / | | | || | __ | | | ||  __|    | |   / _ \   | |  
| |/ / | |\ \ \ \_/ /| |_\ \| |_| || |___   _| |_ | (_) |  | |  
|___/  \_| \_| \___/  \____/ \___/ \____/   \___/  \___/   \_/  
Drogue IoT {} - {} {} ({})
"#, drogue_doppelgaenger_common::version::VERSION, NAME, VERSION, DESCRIPTION);

        std::io::stdout().flush().ok();

        $crate::app::init_tracing(NAME);

        return $run;
    }};
}

pub fn init() {
    dotenv::dotenv().ok();
}

#[cfg(feature = "jaeger")]
fn enable_tracing() -> bool {
    std::env::var("ENABLE_TRACING")
        .ok()
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or_default()
}

#[cfg(feature = "jaeger")]
pub fn init_tracing(name: &str) {
    if !enable_tracing() {
        init_no_tracing();
        return;
    }

    use crate::tracing::{self, tracing_subscriber::prelude::*};

    opentelemetry::global::set_text_map_propagator(
        opentelemetry::sdk::propagation::TraceContextPropagator::new(),
    );
    let pipeline = opentelemetry_jaeger::new_pipeline()
        .with_service_name(name)
        .with_trace_config(opentelemetry::sdk::trace::Config::default().with_sampler(
            opentelemetry::sdk::trace::Sampler::ParentBased(Box::new(tracing::sampler())),
        ));

    println!("Using Jaeger tracing.");
    println!("{:#?}", pipeline);
    println!("Tracing is enabled. This console will not show any logging information.");

    let tracer = pipeline
        .install_batch(tracing::opentelemetry::runtime::Tokio)
        .unwrap();

    tracing_subscriber::Registry::default()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .init();
}

#[cfg(not(feature = "jaeger"))]
#[allow(unused)]
pub fn init_tracing(_: &str) {
    init_no_tracing()
}

#[allow(unused)]
fn init_no_tracing() {
    env_logger::builder().format_timestamp_millis().init();
    log::info!("Tracing is not enabled");
}

/// Run a standard main loop.
pub async fn run_main<'m, M>(main: M) -> anyhow::Result<()>
where
    M: IntoIterator<Item = LocalBoxFuture<'m, Result<(), anyhow::Error>>>,
{
    let mut futures = FuturesUnordered::<LocalBoxFuture<Result<(), anyhow::Error>>>::new();
    futures.extend(main);

    let (result, _, _) = futures::future::select_all(futures).await;

    log::warn!("One of the main runners returned: {result:?}");
    log::warn!("Exiting application...");

    Ok(())
}
