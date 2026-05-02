use tracing::Subscriber;
use tracing_subscriber::layer::Layer;

pub mod console;
pub mod file;
pub mod opentelemetry;

/// Boxed layer type that can wrap any concrete subscriber layer.
pub type BoxLayer<S> = Box<dyn Layer<S> + Send + Sync>;

/// Layer that performs no work, used to keep layering consistent when a sink is disabled.
pub struct NoopLayer;

impl<S> Layer<S> for NoopLayer where S: Subscriber {}

/// Produce a boxed no-op layer matching any subscriber.
pub fn noop_layer<S>() -> BoxLayer<S>
where
    S: Subscriber + 'static,
{
    Box::new(NoopLayer)
}
