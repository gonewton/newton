use crate::logging::layers::BoxLayer;
use anyhow::{Context, Result};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{resource::Resource, trace::SdkTracerProvider};
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::registry::LookupSpan;
use url::Url;

pub struct OpenTelemetryGuard(SdkTracerProvider);

impl OpenTelemetryGuard {
    pub fn new(provider: SdkTracerProvider) -> Self {
        Self(provider)
    }
}

impl Drop for OpenTelemetryGuard {
    fn drop(&mut self) {
        let _ = self.0.force_flush();
        let _ = self.0.shutdown();
    }
}

/// Builds an OpenTelemetry layer wired to the configured OTLP endpoint.
pub fn build_opentelemetry_layer<S>(
    endpoint: &Url,
    service_name: Option<&str>,
) -> Result<(BoxLayer<S>, OpenTelemetryGuard)>
where
    S: Subscriber + for<'span> LookupSpan<'span> + Send + Sync + 'static,
{
    let exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint.as_str())
        .build()
        .context("failed to build OTLP exporter")?;

    let service_name_owned = service_name
        .map(|value| value.to_string())
        .unwrap_or_else(|| "newton".to_string());
    let resource = Resource::builder()
        .with_service_name(service_name_owned)
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();

    let tracer = provider.tracer("newton");
    let layer = OpenTelemetryLayer::new(tracer);

    Ok((Box::new(layer), OpenTelemetryGuard::new(provider)))
}
