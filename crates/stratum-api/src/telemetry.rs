//! Process-wide tracing initialization and OTLP trace export.
//!
//! The fmt subscriber is always installed. When `OTEL_EXPORTER_OTLP_ENDPOINT`
//! is set in the environment, an OpenTelemetry tracer provider with a
//! batching OTLP/HTTP span exporter is layered alongside it so the
//! HTTP → turn → LLM span chain flows to the collector; when unset, behavior
//! is exactly the fmt-only subscriber. `stratum-api` (the binary entry point)
//! is the only crate allowed to install a global subscriber (§4).

use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

use crate::HostError;

/// Handle that flushes and ends the OTLP tracer provider on shutdown.
///
/// Without OTLP activation this carries no state and `shutdown` is a no-op.
#[derive(Debug, Default)]
pub struct TelemetryGuard {
    provider: Option<SdkTracerProvider>,
}

impl TelemetryGuard {
    /// Flushes pending spans and ends the tracer provider, when installed.
    pub fn shutdown(self) {
        if let Some(provider) = self.provider
            && provider.shutdown().is_err()
        {
            tracing::warn!("otlp tracer provider shutdown failed");
        }
    }
}

/// Installs the global tracing subscriber: the fmt layer always, plus an OTLP
/// span layer when `OTEL_EXPORTER_OTLP_ENDPOINT` is set.
///
/// # Errors
///
/// Returns [`HostError::Telemetry`] when the OTLP exporter cannot be built or
/// the global subscriber was already installed.
pub fn init_telemetry() -> Result<TelemetryGuard, HostError> {
    if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_none() {
        fmt()
            .with_env_filter(EnvFilter::from_default_env())
            .try_init()
            .map_err(HostError::Telemetry)?;
        return Ok(TelemetryGuard::default());
    }

    // The exporter reads endpoint/protocol from the standard OTEL_* env vars;
    // only the HTTP (protobuf over reqwest-blocking) transport is compiled in.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .map_err(|source| HostError::Telemetry(Box::new(source)))?;
    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();
    let otel_layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("stratum-api"));
    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(fmt::layer())
        .with(otel_layer)
        .try_init()
        .map_err(|source| HostError::Telemetry(Box::new(source)))?;
    Ok(TelemetryGuard {
        provider: Some(provider),
    })
}
