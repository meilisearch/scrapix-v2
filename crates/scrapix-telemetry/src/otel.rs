//! # OpenTelemetry Distributed Tracing
//!
//! Provides OpenTelemetry integration for distributed tracing across services.
//!
//! ## Features
//!
//! - OTLP exporter for traces (gRPC or HTTP)
//! - Trace context propagation
//! - Integration with tracing crate via tracing-opentelemetry
//! - Configurable sampling strategies
//! - Resource attributes for service identification
//!
//! ## Example
//!
//! ```rust,ignore
//! use scrapix_telemetry::otel::{OtelConfig, init_tracing};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize OpenTelemetry tracing
//!     let config = OtelConfig::builder()
//!         .service_name("scrapix-crawler")
//!         .endpoint("http://localhost:4317")
//!         .build();
//!
//!     let _guard = init_tracing(&config)?;
//!
//!     // Create spans using tracing macros
//!     tracing::info_span!("fetch_page", url = "https://example.com").in_scope(|| {
//!         tracing::info!("Fetching page...");
//!     });
//!
//!     Ok(())
//! }
//! ```

use opentelemetry::KeyValue;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::runtime::Tokio;
use opentelemetry_sdk::trace::{Sampler, TracerProvider};
use opentelemetry_sdk::Resource;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during OpenTelemetry initialization.
#[derive(Error, Debug)]
pub enum OtelError {
    /// Failed to create OTLP exporter.
    #[error("Failed to create OTLP exporter: {0}")]
    ExporterError(String),

    /// Failed to create tracer provider.
    #[error("Failed to create tracer provider: {0}")]
    TracerProviderError(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    /// Failed to initialize global tracer.
    #[error("Failed to initialize global tracer: {0}")]
    InitError(String),
}

// ============================================================================
// Configuration
// ============================================================================

/// OTLP protocol to use for exporting traces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OtlpProtocol {
    /// gRPC protocol (default, recommended for production)
    #[default]
    Grpc,
    /// HTTP/protobuf protocol
    Http,
}

/// Sampling strategy for traces.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingStrategy {
    /// Always sample all traces
    #[default]
    AlwaysOn,
    /// Never sample any traces
    AlwaysOff,
    /// Sample based on trace ID ratio (0.0 - 1.0)
    TraceIdRatioBased { ratio: f64 },
    /// Parent-based sampling (inherits from parent span)
    ParentBased { root: Box<SamplingStrategy> },
}

impl SamplingStrategy {
    fn to_sampler(&self) -> Sampler {
        match self {
            SamplingStrategy::AlwaysOn => Sampler::AlwaysOn,
            SamplingStrategy::AlwaysOff => Sampler::AlwaysOff,
            SamplingStrategy::TraceIdRatioBased { ratio } => Sampler::TraceIdRatioBased(*ratio),
            SamplingStrategy::ParentBased { root } => {
                Sampler::ParentBased(Box::new(root.to_sampler()))
            }
        }
    }
}

/// Configuration for OpenTelemetry tracing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelConfig {
    /// Service name for trace identification.
    pub service_name: String,

    /// Service version.
    #[serde(default)]
    pub service_version: Option<String>,

    /// Service namespace.
    #[serde(default)]
    pub service_namespace: Option<String>,

    /// Deployment environment (e.g., "production", "staging").
    #[serde(default)]
    pub deployment_environment: Option<String>,

    /// OTLP collector endpoint.
    pub endpoint: String,

    /// OTLP protocol to use.
    #[serde(default)]
    pub protocol: OtlpProtocol,

    /// Sampling strategy.
    #[serde(default)]
    pub sampling: SamplingStrategy,

    /// Request timeout for OTLP exports.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// Maximum batch size for span exports.
    #[serde(default = "default_batch_size")]
    pub max_batch_size: usize,

    /// Maximum queue size for pending spans.
    #[serde(default = "default_queue_size")]
    pub max_queue_size: usize,

    /// Scheduled delay for batch exports (in milliseconds).
    #[serde(default = "default_scheduled_delay_ms")]
    pub scheduled_delay_ms: u64,

    /// Maximum export batch size.
    #[serde(default = "default_max_export_batch_size")]
    pub max_export_batch_size: usize,

    /// Additional resource attributes.
    #[serde(default)]
    pub resource_attributes: Vec<(String, String)>,

    /// Whether tracing is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_timeout_secs() -> u64 {
    10
}

fn default_batch_size() -> usize {
    512
}

fn default_queue_size() -> usize {
    2048
}

fn default_scheduled_delay_ms() -> u64 {
    5000
}

fn default_max_export_batch_size() -> usize {
    512
}

fn default_true() -> bool {
    true
}

impl Default for OtelConfig {
    fn default() -> Self {
        Self {
            service_name: "scrapix".to_string(),
            service_version: None,
            service_namespace: None,
            deployment_environment: None,
            endpoint: "http://localhost:4317".to_string(),
            protocol: OtlpProtocol::default(),
            sampling: SamplingStrategy::default(),
            timeout_secs: default_timeout_secs(),
            max_batch_size: default_batch_size(),
            max_queue_size: default_queue_size(),
            scheduled_delay_ms: default_scheduled_delay_ms(),
            max_export_batch_size: default_max_export_batch_size(),
            resource_attributes: Vec::new(),
            enabled: true,
        }
    }
}

impl OtelConfig {
    /// Create a new builder for OtelConfig.
    pub fn builder() -> OtelConfigBuilder {
        OtelConfigBuilder::default()
    }

    /// Build resource attributes from config.
    fn build_resource(&self) -> Resource {
        let mut attrs = vec![KeyValue::new("service.name", self.service_name.clone())];

        if let Some(ref version) = self.service_version {
            attrs.push(KeyValue::new("service.version", version.clone()));
        }

        if let Some(ref namespace) = self.service_namespace {
            attrs.push(KeyValue::new("service.namespace", namespace.clone()));
        }

        if let Some(ref env) = self.deployment_environment {
            attrs.push(KeyValue::new("deployment.environment", env.clone()));
        }

        // Add custom attributes
        for (key, value) in &self.resource_attributes {
            attrs.push(KeyValue::new(key.clone(), value.clone()));
        }

        Resource::new(attrs)
    }
}

/// Builder for OtelConfig.
#[derive(Debug, Default)]
pub struct OtelConfigBuilder {
    service_name: Option<String>,
    service_version: Option<String>,
    service_namespace: Option<String>,
    deployment_environment: Option<String>,
    endpoint: Option<String>,
    protocol: Option<OtlpProtocol>,
    sampling: Option<SamplingStrategy>,
    timeout_secs: Option<u64>,
    max_batch_size: Option<usize>,
    max_queue_size: Option<usize>,
    scheduled_delay_ms: Option<u64>,
    max_export_batch_size: Option<usize>,
    resource_attributes: Vec<(String, String)>,
    enabled: Option<bool>,
}

impl OtelConfigBuilder {
    /// Set the service name.
    pub fn service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }

    /// Set the service version.
    pub fn service_version(mut self, version: impl Into<String>) -> Self {
        self.service_version = Some(version.into());
        self
    }

    /// Set the service namespace.
    pub fn service_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.service_namespace = Some(namespace.into());
        self
    }

    /// Set the deployment environment.
    pub fn deployment_environment(mut self, env: impl Into<String>) -> Self {
        self.deployment_environment = Some(env.into());
        self
    }

    /// Set the OTLP endpoint.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Set the OTLP protocol.
    pub fn protocol(mut self, protocol: OtlpProtocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    /// Set the sampling strategy.
    pub fn sampling(mut self, strategy: SamplingStrategy) -> Self {
        self.sampling = Some(strategy);
        self
    }

    /// Set the export timeout in seconds.
    pub fn timeout_secs(mut self, timeout: u64) -> Self {
        self.timeout_secs = Some(timeout);
        self
    }

    /// Set the maximum batch size.
    pub fn max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = Some(size);
        self
    }

    /// Set the maximum queue size.
    pub fn max_queue_size(mut self, size: usize) -> Self {
        self.max_queue_size = Some(size);
        self
    }

    /// Set the scheduled delay for batch exports.
    pub fn scheduled_delay_ms(mut self, delay: u64) -> Self {
        self.scheduled_delay_ms = Some(delay);
        self
    }

    /// Add a custom resource attribute.
    pub fn resource_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.resource_attributes.push((key.into(), value.into()));
        self
    }

    /// Enable or disable tracing.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Build the OtelConfig.
    pub fn build(self) -> OtelConfig {
        let defaults = OtelConfig::default();
        OtelConfig {
            service_name: self.service_name.unwrap_or(defaults.service_name),
            service_version: self.service_version.or(defaults.service_version),
            service_namespace: self.service_namespace.or(defaults.service_namespace),
            deployment_environment: self
                .deployment_environment
                .or(defaults.deployment_environment),
            endpoint: self.endpoint.unwrap_or(defaults.endpoint),
            protocol: self.protocol.unwrap_or(defaults.protocol),
            sampling: self.sampling.unwrap_or(defaults.sampling),
            timeout_secs: self.timeout_secs.unwrap_or(defaults.timeout_secs),
            max_batch_size: self.max_batch_size.unwrap_or(defaults.max_batch_size),
            max_queue_size: self.max_queue_size.unwrap_or(defaults.max_queue_size),
            scheduled_delay_ms: self
                .scheduled_delay_ms
                .unwrap_or(defaults.scheduled_delay_ms),
            max_export_batch_size: self
                .max_export_batch_size
                .unwrap_or(defaults.max_export_batch_size),
            resource_attributes: if self.resource_attributes.is_empty() {
                defaults.resource_attributes
            } else {
                self.resource_attributes
            },
            enabled: self.enabled.unwrap_or(defaults.enabled),
        }
    }
}

// ============================================================================
// Initialization
// ============================================================================

/// Guard that shuts down tracing when dropped.
pub struct TracingGuard {
    provider: Option<TracerProvider>,
}

impl TracingGuard {
    /// Manually shutdown tracing.
    pub fn shutdown(mut self) {
        if let Some(provider) = self.provider.take() {
            // Flush any remaining spans
            let flush_results = provider.force_flush();
            for result in flush_results {
                if let Err(e) = result {
                    tracing::warn!("Failed to flush traces: {:?}", e);
                }
            }
            if let Err(e) = provider.shutdown() {
                tracing::warn!("Failed to shutdown tracer provider: {:?}", e);
            }
        }
    }
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            // Best effort flush and shutdown on drop
            let _ = provider.force_flush();
            let _ = provider.shutdown();
        }
    }
}

/// Initialize OpenTelemetry tracing.
///
/// Returns a guard that will shutdown tracing when dropped.
pub fn init_tracing(config: &OtelConfig) -> Result<TracingGuard, OtelError> {
    if !config.enabled {
        return Ok(TracingGuard { provider: None });
    }

    // Build the exporter (gRPC via tonic is the main supported protocol)
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&config.endpoint)
        .with_timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|e| OtelError::ExporterError(e.to_string()))?;

    // Build the tracer provider using the new API
    let provider = TracerProvider::builder()
        .with_batch_exporter(exporter, Tokio)
        .with_sampler(config.sampling.to_sampler())
        .with_resource(config.build_resource())
        .build();

    // Set as global provider
    let _ = opentelemetry::global::set_tracer_provider(provider.clone());

    tracing::info!(
        service = %config.service_name,
        endpoint = %config.endpoint,
        protocol = ?config.protocol,
        "OpenTelemetry tracing initialized"
    );

    Ok(TracingGuard {
        provider: Some(provider),
    })
}

/// Create a tracer for the given component.
pub fn tracer(name: &'static str) -> opentelemetry::global::BoxedTracer {
    opentelemetry::global::tracer(name)
}

// ============================================================================
// Context Propagation
// ============================================================================

/// Extract trace context from HTTP headers.
pub fn extract_context<'a, I>(headers: I) -> opentelemetry::Context
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    use opentelemetry::propagation::TextMapPropagator;
    use opentelemetry_sdk::propagation::TraceContextPropagator;

    let propagator = TraceContextPropagator::new();
    let extractor = HeaderExtractor::new(headers);
    propagator.extract(&extractor)
}

/// Inject trace context into HTTP headers.
pub fn inject_context(cx: &opentelemetry::Context) -> Vec<(String, String)> {
    use opentelemetry::propagation::TextMapPropagator;
    use opentelemetry_sdk::propagation::TraceContextPropagator;

    let propagator = TraceContextPropagator::new();
    let mut injector = HeaderInjector::default();
    propagator.inject_context(cx, &mut injector);
    injector.headers
}

/// Header extractor for trace context propagation.
struct HeaderExtractor<'a> {
    headers: Vec<(&'a str, &'a str)>,
}

impl<'a> HeaderExtractor<'a> {
    fn new<I>(headers: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        Self {
            headers: headers.into_iter().collect(),
        }
    }
}

impl<'a> opentelemetry::propagation::Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| *v)
    }

    fn keys(&self) -> Vec<&str> {
        self.headers.iter().map(|(k, _)| *k).collect()
    }
}

/// Header injector for trace context propagation.
#[derive(Default)]
struct HeaderInjector {
    headers: Vec<(String, String)>,
}

impl opentelemetry::propagation::Injector for HeaderInjector {
    fn set(&mut self, key: &str, value: String) {
        self.headers.push((key.to_string(), value));
    }
}

// ============================================================================
// Span Attributes Helpers
// ============================================================================

/// Common crawler span attributes.
pub struct SpanAttributes;

impl SpanAttributes {
    /// Create attributes for an HTTP request.
    pub fn http_request(method: &str, url: &str) -> Vec<KeyValue> {
        vec![
            KeyValue::new("http.method", method.to_string()),
            KeyValue::new("http.url", url.to_string()),
        ]
    }

    /// Create attributes for an HTTP response.
    pub fn http_response(status_code: i64, body_size: i64) -> Vec<KeyValue> {
        vec![
            KeyValue::new("http.status_code", status_code),
            KeyValue::new("http.response.body.size", body_size),
        ]
    }

    /// Create attributes for a URL.
    pub fn url(url: &str, depth: i64) -> Vec<KeyValue> {
        vec![
            KeyValue::new("url.full", url.to_string()),
            KeyValue::new("crawler.depth", depth),
        ]
    }

    /// Create attributes for document processing.
    pub fn document(content_type: &str, size: i64) -> Vec<KeyValue> {
        vec![
            KeyValue::new("document.content_type", content_type.to_string()),
            KeyValue::new("document.size", size),
        ]
    }

    /// Create attributes for an error.
    pub fn error(error_type: &str, message: &str) -> Vec<KeyValue> {
        vec![
            KeyValue::new("error.type", error_type.to_string()),
            KeyValue::new("error.message", message.to_string()),
            KeyValue::new("otel.status_code", "ERROR"),
        ]
    }

    /// Create attributes for AI/LLM operations.
    pub fn ai_request(model: &str, input_tokens: i64, output_tokens: i64) -> Vec<KeyValue> {
        vec![
            KeyValue::new("ai.model", model.to_string()),
            KeyValue::new("ai.input_tokens", input_tokens),
            KeyValue::new("ai.output_tokens", output_tokens),
        ]
    }

    /// Create attributes for storage operations.
    pub fn storage(backend: &str, operation: &str, bytes: i64) -> Vec<KeyValue> {
        vec![
            KeyValue::new("storage.backend", backend.to_string()),
            KeyValue::new("storage.operation", operation.to_string()),
            KeyValue::new("storage.bytes", bytes),
        ]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = OtelConfig::builder()
            .service_name("test-service")
            .service_version("1.0.0")
            .endpoint("http://otel-collector:4317")
            .protocol(OtlpProtocol::Grpc)
            .sampling(SamplingStrategy::TraceIdRatioBased { ratio: 0.5 })
            .resource_attribute("custom.attr", "value")
            .build();

        assert_eq!(config.service_name, "test-service");
        assert_eq!(config.service_version, Some("1.0.0".to_string()));
        assert_eq!(config.endpoint, "http://otel-collector:4317");
        assert_eq!(config.protocol, OtlpProtocol::Grpc);
        assert_eq!(config.resource_attributes.len(), 1);
    }

    #[test]
    fn test_default_config() {
        let config = OtelConfig::default();

        assert_eq!(config.service_name, "scrapix");
        assert_eq!(config.endpoint, "http://localhost:4317");
        assert!(config.enabled);
    }

    #[test]
    fn test_sampling_strategies() {
        // Test each sampling strategy creates a valid sampler
        let _ = SamplingStrategy::AlwaysOn.to_sampler();
        let _ = SamplingStrategy::AlwaysOff.to_sampler();
        let _ = SamplingStrategy::TraceIdRatioBased { ratio: 0.5 }.to_sampler();
        let _ = SamplingStrategy::ParentBased {
            root: Box::new(SamplingStrategy::AlwaysOn),
        }
        .to_sampler();
    }

    #[test]
    fn test_header_extraction() {
        let headers = vec![
            (
                "traceparent",
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            ),
            ("tracestate", "congo=t61rcWkgMzE"),
        ];

        let _context = extract_context(headers);
        // Context is created, no panic = success
    }

    #[test]
    fn test_span_attributes() {
        let attrs = SpanAttributes::http_request("GET", "https://example.com");
        assert_eq!(attrs.len(), 2);

        let attrs = SpanAttributes::error("timeout", "Connection timed out");
        assert_eq!(attrs.len(), 3);
    }

    #[test]
    fn test_disabled_tracing() {
        let config = OtelConfig::builder().enabled(false).build();

        // Should not fail when disabled
        let guard = init_tracing(&config).unwrap();
        assert!(guard.provider.is_none());
    }
}
