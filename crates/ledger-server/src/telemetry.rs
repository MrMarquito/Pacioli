use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::TracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Registry};

pub fn init_telemetry(service_name: &'static str) {
    let provider = TracerProvider::builder().build();
    let tracer = provider.tracer(service_name);
    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    let filter_layer =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));

    let formatting_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true);

    Registry::default()
        .with(filter_layer)
        .with(formatting_layer)
        .with(telemetry_layer)
        .init();
}
