//! # Scrapix Telemetry
//!
//! Observability and telemetry for Scrapix web crawler.
//!
//! This crate provides a unified telemetry stack:
//! - **Structured logging** via tracing with configurable formatters
//! - **Distributed tracing** via OpenTelemetry with OTLP export
//! - **Prometheus metrics** with pre-defined crawler metrics
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use scrapix_telemetry::{TelemetryConfig, init_telemetry};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize all telemetry with defaults
//!     let config = TelemetryConfig::default();
//!     let _guard = init_telemetry(&config).await?;
//!
//!     // Use tracing macros
//!     tracing::info!("Application started");
//!
//!     // Record metrics
//!     scrapix_telemetry::metrics::CrawlerMetrics::record_page_fetched("example.com", 200);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Individual Components
//!
//! You can also initialize components individually:
//!
//! ```rust,ignore
//! use scrapix_telemetry::logging::{LogConfig, init_logging};
//! use scrapix_telemetry::metrics::{MetricsConfig, MetricsExporter};
//! use scrapix_telemetry::otel::{OtelConfig, init_tracing};
//!
//! fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize just logging
//!     let log_config = LogConfig::development();
//!     init_logging(&log_config)?;
//!
//!     tracing::info!("Logging initialized");
//!     Ok(())
//! }
//! ```

pub mod logging;
pub mod metrics;
pub mod otel;

// Re-export main types for convenience
pub use logging::{LogConfig, LogConfigBuilder, LogFormat, LogTarget, LoggingError};
pub use metrics::{
    CrawlerMetrics, MetricsConfig, MetricsConfigBuilder, MetricsError, MetricsExporter,
    MetricsServerHandle, TimingGuard,
};
pub use otel::{
    extract_context, inject_context, tracer, OtelConfig, OtelConfigBuilder, OtelError,
    OtlpProtocol, SamplingStrategy, SpanAttributes, TracingGuard,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

// ============================================================================
// Unified Telemetry Error
// ============================================================================

/// Errors that can occur during telemetry initialization.
#[derive(Error, Debug)]
pub enum TelemetryError {
    /// Logging initialization failed.
    #[error("Logging initialization failed: {0}")]
    LoggingError(#[from] LoggingError),

    /// Metrics initialization failed.
    #[error("Metrics initialization failed: {0}")]
    MetricsError(#[from] MetricsError),

    /// OpenTelemetry initialization failed.
    #[error("OpenTelemetry initialization failed: {0}")]
    OtelError(#[from] OtelError),
}

// ============================================================================
// Unified Configuration
// ============================================================================

/// Unified telemetry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Logging configuration.
    #[serde(default)]
    pub logging: LogConfig,

    /// Metrics configuration.
    #[serde(default)]
    pub metrics: MetricsConfig,

    /// OpenTelemetry tracing configuration.
    #[serde(default)]
    pub otel: OtelConfig,

    /// Whether to enable logging.
    #[serde(default = "default_true")]
    pub enable_logging: bool,

    /// Whether to enable metrics.
    #[serde(default = "default_true")]
    pub enable_metrics: bool,

    /// Whether to enable OpenTelemetry tracing.
    #[serde(default)]
    pub enable_otel: bool,
}

fn default_true() -> bool {
    true
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            logging: LogConfig::default(),
            metrics: MetricsConfig::default(),
            otel: OtelConfig::default(),
            enable_logging: true,
            enable_metrics: true,
            enable_otel: false, // Disabled by default - requires collector
        }
    }
}

impl TelemetryConfig {
    /// Create a new builder for TelemetryConfig.
    pub fn builder() -> TelemetryConfigBuilder {
        TelemetryConfigBuilder::default()
    }

    /// Create a development configuration.
    ///
    /// - Pretty logging with debug level
    /// - Metrics enabled
    /// - OpenTelemetry disabled
    pub fn development() -> Self {
        Self {
            logging: LogConfig::development(),
            metrics: MetricsConfig::default(),
            otel: OtelConfig::default(),
            enable_logging: true,
            enable_metrics: true,
            enable_otel: false,
        }
    }

    /// Create a production configuration.
    ///
    /// - JSON logging with info level
    /// - Metrics enabled
    /// - OpenTelemetry enabled
    pub fn production(service_name: impl Into<String>) -> Self {
        let service = service_name.into();
        Self {
            logging: LogConfig::production(),
            metrics: MetricsConfig::builder()
                .global_label("service", &service)
                .build(),
            otel: OtelConfig::builder().service_name(&service).build(),
            enable_logging: true,
            enable_metrics: true,
            enable_otel: true,
        }
    }
}

/// Builder for TelemetryConfig.
#[derive(Debug, Default)]
pub struct TelemetryConfigBuilder {
    logging: Option<LogConfig>,
    metrics: Option<MetricsConfig>,
    otel: Option<OtelConfig>,
    enable_logging: Option<bool>,
    enable_metrics: Option<bool>,
    enable_otel: Option<bool>,
}

impl TelemetryConfigBuilder {
    /// Set the logging configuration.
    pub fn logging(mut self, config: LogConfig) -> Self {
        self.logging = Some(config);
        self
    }

    /// Set the metrics configuration.
    pub fn metrics(mut self, config: MetricsConfig) -> Self {
        self.metrics = Some(config);
        self
    }

    /// Set the OpenTelemetry configuration.
    pub fn otel(mut self, config: OtelConfig) -> Self {
        self.otel = Some(config);
        self
    }

    /// Enable or disable logging.
    pub fn enable_logging(mut self, enabled: bool) -> Self {
        self.enable_logging = Some(enabled);
        self
    }

    /// Enable or disable metrics.
    pub fn enable_metrics(mut self, enabled: bool) -> Self {
        self.enable_metrics = Some(enabled);
        self
    }

    /// Enable or disable OpenTelemetry tracing.
    pub fn enable_otel(mut self, enabled: bool) -> Self {
        self.enable_otel = Some(enabled);
        self
    }

    /// Build the TelemetryConfig.
    pub fn build(self) -> TelemetryConfig {
        let defaults = TelemetryConfig::default();
        TelemetryConfig {
            logging: self.logging.unwrap_or(defaults.logging),
            metrics: self.metrics.unwrap_or(defaults.metrics),
            otel: self.otel.unwrap_or(defaults.otel),
            enable_logging: self.enable_logging.unwrap_or(defaults.enable_logging),
            enable_metrics: self.enable_metrics.unwrap_or(defaults.enable_metrics),
            enable_otel: self.enable_otel.unwrap_or(defaults.enable_otel),
        }
    }
}

// ============================================================================
// Unified Telemetry Guard
// ============================================================================

/// Guard that manages telemetry lifecycle.
///
/// When dropped, shuts down all telemetry components gracefully.
pub struct TelemetryGuard {
    otel_guard: Option<TracingGuard>,
    metrics_handle: Option<MetricsServerHandle>,
}

impl TelemetryGuard {
    /// Manually shutdown all telemetry.
    pub async fn shutdown(mut self) {
        // Shutdown OpenTelemetry first to flush traces
        if let Some(guard) = self.otel_guard.take() {
            guard.shutdown();
        }

        // Shutdown metrics server
        if let Some(handle) = self.metrics_handle.take() {
            handle.shutdown().await;
        }
    }
}

// ============================================================================
// Initialization
// ============================================================================

/// Initialize all telemetry components.
///
/// Returns a guard that will shutdown telemetry when dropped.
pub async fn init_telemetry(config: &TelemetryConfig) -> Result<TelemetryGuard, TelemetryError> {
    // Initialize logging first
    if config.enable_logging {
        logging::init_logging(&config.logging)?;
        tracing::debug!("Logging initialized");
    }

    // Initialize OpenTelemetry
    let otel_guard = if config.enable_otel {
        let guard = otel::init_tracing(&config.otel)?;
        tracing::debug!("OpenTelemetry tracing initialized");
        Some(guard)
    } else {
        None
    };

    // Initialize metrics
    let metrics_handle = if config.enable_metrics {
        let exporter = MetricsExporter::new(config.metrics.clone())?.install()?;
        let handle = exporter.start().await?;
        tracing::debug!(
            addr = %config.metrics.listen_addr,
            "Metrics server started"
        );
        Some(handle)
    } else {
        None
    };

    tracing::info!("Telemetry initialized");

    Ok(TelemetryGuard {
        otel_guard,
        metrics_handle,
    })
}

/// Initialize only logging (sync, suitable for simple applications).
pub fn init_logging_only(config: &LogConfig) -> Result<(), TelemetryError> {
    logging::init_logging(config)?;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TelemetryConfig::default();
        assert!(config.enable_logging);
        assert!(config.enable_metrics);
        assert!(!config.enable_otel);
    }

    #[test]
    fn test_development_config() {
        let config = TelemetryConfig::development();
        assert!(config.enable_logging);
        assert!(config.enable_metrics);
        assert!(!config.enable_otel);
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_production_config() {
        let config = TelemetryConfig::production("test-service");
        assert!(config.enable_logging);
        assert!(config.enable_metrics);
        assert!(config.enable_otel);
        assert_eq!(config.otel.service_name, "test-service");
    }

    #[test]
    fn test_builder() {
        let config = TelemetryConfig::builder()
            .enable_logging(true)
            .enable_metrics(false)
            .enable_otel(false)
            .build();

        assert!(config.enable_logging);
        assert!(!config.enable_metrics);
        assert!(!config.enable_otel);
    }
}
