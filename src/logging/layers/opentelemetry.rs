use crate::logging::config::OpenTelemetryConfig;
use crate::Result;
use anyhow::{anyhow, Context};
use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use opentelemetry_sdk::Resource;
use tracing::Subscriber;
use tracing_core::{
    dispatcher::Dispatch,
    span::{self, Id, Record},
    Event, Interest, Metadata,
};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::{Context as LayerContext, Layer};
use tracing_subscriber::registry::LookupSpan;

#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

/// Handle that keeps the configured tracer provider alive for shutdown.
pub struct OpenTelemetryHandle {
    provider: SdkTracerProvider,
}

impl OpenTelemetryHandle {
    fn new(provider: SdkTracerProvider) -> Self {
        Self { provider }
    }

    pub(crate) fn shutdown(self) {
        let _ = self.provider.shutdown();
    }
}

/// Generic optional layer wrapper so we always add a layer regardless of configuration.
pub enum OptionalLayer<L> {
    Enabled(L),
    Disabled,
}

impl<L> OptionalLayer<L> {
    pub fn enabled(layer: L) -> Self {
        Self::Enabled(layer)
    }

    pub fn disabled() -> Self {
        Self::Disabled
    }
}

impl<S, L> Layer<S> for OptionalLayer<L>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    L: Layer<S>,
{
    fn on_register_dispatch(&self, subscriber: &Dispatch) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_register_dispatch(subscriber);
        }
    }

    fn on_layer(&mut self, subscriber: &mut S) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_layer(subscriber);
        }
    }

    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        match self {
            OptionalLayer::Enabled(layer) => layer.register_callsite(metadata),
            OptionalLayer::Disabled => Interest::always(),
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>, ctx: LayerContext<'_, S>) -> bool {
        match self {
            OptionalLayer::Enabled(layer) => layer.enabled(metadata, ctx),
            OptionalLayer::Disabled => true,
        }
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        match self {
            OptionalLayer::Enabled(layer) => layer.max_level_hint(),
            OptionalLayer::Disabled => None,
        }
    }

    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &Id, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_new_span(attrs, id, ctx);
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_record(id, values, ctx);
        }
    }

    fn on_follows_from(&self, span: &Id, follows: &Id, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_follows_from(span, follows, ctx);
        }
    }

    fn event_enabled(&self, event: &Event<'_>, ctx: LayerContext<'_, S>) -> bool {
        match self {
            OptionalLayer::Enabled(layer) => layer.event_enabled(event, ctx),
            OptionalLayer::Disabled => true,
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_event(event, ctx);
        }
    }

    fn on_enter(&self, id: &Id, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_enter(id, ctx);
        }
    }

    fn on_exit(&self, id: &Id, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_exit(id, ctx);
        }
    }

    fn on_close(&self, id: Id, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_close(id, ctx);
        }
    }

    fn on_id_change(&self, old: &Id, new: &Id, ctx: LayerContext<'_, S>) {
        if let OptionalLayer::Enabled(layer) = self {
            layer.on_id_change(old, new, ctx);
        }
    }
}

/// Initialize the OpenTelemetry export pipeline.
///
/// A warning is emitted by the caller if initialization fails so the application can continue
/// emitting to non-OTel sinks.
pub fn init<S>(
    config: &OpenTelemetryConfig,
) -> Result<(OpenTelemetryLayer<S, SdkTracer>, OpenTelemetryHandle)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    #[cfg(test)]
    {
        if FORCE_FAILURE.load(Ordering::SeqCst) {
            return Err(anyhow!("simulated OpenTelemetry failure"));
        }
    }

    let endpoint = config
        .endpoint
        .as_deref()
        .ok_or_else(|| anyhow!("OpenTelemetry endpoint missing"))?;

    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint)
        .with_protocol(Protocol::HttpBinary)
        .build()
        .context("failed to build OTLP exporter")?;

    let resource = Resource::builder_empty()
        .with_service_name(config.service_name.clone())
        .build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer(env!("CARGO_PKG_NAME"));

    global::set_tracer_provider(provider.clone());

    let layer = OpenTelemetryLayer::new(tracer);
    Ok((layer, OpenTelemetryHandle::new(provider)))
}

#[cfg(test)]
static FORCE_FAILURE: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
pub fn simulate_failure(value: bool) {
    FORCE_FAILURE.store(value, Ordering::SeqCst);
}
