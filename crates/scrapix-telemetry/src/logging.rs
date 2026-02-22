//! Structured logging with tracing-subscriber
//!
//! This module provides structured logging capabilities for Scrapix:
//! - JSON or pretty console output
//! - Configurable log levels per module
//! - Environment-based filtering
//! - File logging with rotation
//!
//! ## Usage
//!
//! ```rust,no_run
//! use scrapix_telemetry::logging::{LogConfig, init_logging};
//!
//! fn main() {
//!     let config = LogConfig::default();
//!     init_logging(&config).expect("Failed to initialize logging");
//!
//!     tracing::info!("Application started");
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use thiserror::Error;
use tracing::Level;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::fmt::time::UtcTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, Layer};

/// Errors that can occur during logging initialization
#[derive(Debug, Error)]
pub enum LoggingError {
    #[error("Failed to initialize logging: {0}")]
    InitError(String),

    #[error("Invalid log level: {0}")]
    InvalidLevel(String),

    #[error("Failed to create log file: {0}")]
    FileError(#[from] io::Error),

    #[error("Failed to set global subscriber: {0}")]
    SubscriberError(String),
}

/// Log output format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Pretty console output (colored, human-readable)
    #[default]
    Pretty,
    /// JSON structured output (for log aggregators)
    Json,
    /// Compact single-line output
    Compact,
    /// Full format with all details
    Full,
}

/// Log output target
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogTarget {
    /// Write to stdout
    #[default]
    Stdout,
    /// Write to stderr
    Stderr,
    /// Write to a file
    File { path: PathBuf },
}

/// Configuration for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogConfig {
    /// Default log level
    #[serde(default = "default_level")]
    pub level: String,

    /// Log output format
    #[serde(default)]
    pub format: LogFormat,

    /// Log output target
    #[serde(default)]
    pub target: LogTarget,

    /// Whether to include timestamps
    #[serde(default = "default_true")]
    pub timestamps: bool,

    /// Whether to include file/line information
    #[serde(default)]
    pub file_line: bool,

    /// Whether to include thread names
    #[serde(default)]
    pub thread_names: bool,

    /// Whether to include thread IDs
    #[serde(default)]
    pub thread_ids: bool,

    /// Whether to include target module
    #[serde(default = "default_true")]
    pub target_module: bool,

    /// Whether to use ANSI colors
    #[serde(default = "default_true")]
    pub ansi_colors: bool,

    /// Environment variable for filter override
    #[serde(default = "default_env_filter")]
    pub env_filter_var: String,

    /// Span events to log (new, close, enter, exit)
    #[serde(default)]
    pub span_events: SpanEventConfig,

    /// Per-module log levels
    #[serde(default)]
    pub module_levels: Vec<ModuleLevel>,
}

fn default_level() -> String {
    "info".to_string()
}

fn default_true() -> bool {
    true
}

fn default_env_filter() -> String {
    "RUST_LOG".to_string()
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: default_level(),
            format: LogFormat::default(),
            target: LogTarget::default(),
            timestamps: true,
            file_line: false,
            thread_names: false,
            thread_ids: false,
            target_module: true,
            ansi_colors: true,
            env_filter_var: default_env_filter(),
            span_events: SpanEventConfig::default(),
            module_levels: Vec::new(),
        }
    }
}

/// Configuration for span events
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpanEventConfig {
    /// Log when spans are entered
    #[serde(default)]
    pub enter: bool,
    /// Log when spans are exited
    #[serde(default)]
    pub exit: bool,
    /// Log when spans are created
    #[serde(default)]
    pub new: bool,
    /// Log when spans are closed
    #[serde(default)]
    pub close: bool,
}

impl SpanEventConfig {
    fn to_fmt_span(&self) -> FmtSpan {
        let mut span = FmtSpan::NONE;
        if self.enter {
            span |= FmtSpan::ENTER;
        }
        if self.exit {
            span |= FmtSpan::EXIT;
        }
        if self.new {
            span |= FmtSpan::NEW;
        }
        if self.close {
            span |= FmtSpan::CLOSE;
        }
        span
    }
}

/// Per-module log level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleLevel {
    /// Module name (e.g., "scrapix_crawler::fetcher")
    pub module: String,
    /// Log level for this module
    pub level: String,
}

impl LogConfig {
    /// Create a new config with the given level
    pub fn with_level(level: impl Into<String>) -> Self {
        Self {
            level: level.into(),
            ..Default::default()
        }
    }

    /// Create a config for development (pretty, debug level)
    pub fn development() -> Self {
        Self {
            level: "debug".to_string(),
            format: LogFormat::Pretty,
            ansi_colors: true,
            file_line: true,
            ..Default::default()
        }
    }

    /// Create a config for production (JSON, info level)
    pub fn production() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::Json,
            ansi_colors: false,
            timestamps: true,
            ..Default::default()
        }
    }

    /// Build the environment filter from config
    fn build_filter(&self) -> Result<EnvFilter, LoggingError> {
        // Start with base filter from environment or config
        let mut filter = match std::env::var(&self.env_filter_var) {
            Ok(env_filter) => EnvFilter::try_new(&env_filter)
                .map_err(|e| LoggingError::InitError(e.to_string()))?,
            Err(_) => EnvFilter::try_new(&self.level)
                .map_err(|e| LoggingError::InitError(e.to_string()))?,
        };

        // Add per-module filters
        for module_level in &self.module_levels {
            let directive = format!("{}={}", module_level.module, module_level.level);
            filter = filter.add_directive(
                directive
                    .parse()
                    .map_err(|e| LoggingError::InitError(format!("Invalid directive: {}", e)))?,
            );
        }

        Ok(filter)
    }
}

/// Builder for LogConfig
pub struct LogConfigBuilder {
    config: LogConfig,
}

impl LogConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: LogConfig::default(),
        }
    }

    pub fn level(mut self, level: impl Into<String>) -> Self {
        self.config.level = level.into();
        self
    }

    pub fn format(mut self, format: LogFormat) -> Self {
        self.config.format = format;
        self
    }

    pub fn target(mut self, target: LogTarget) -> Self {
        self.config.target = target;
        self
    }

    pub fn timestamps(mut self, enabled: bool) -> Self {
        self.config.timestamps = enabled;
        self
    }

    pub fn file_line(mut self, enabled: bool) -> Self {
        self.config.file_line = enabled;
        self
    }

    pub fn thread_names(mut self, enabled: bool) -> Self {
        self.config.thread_names = enabled;
        self
    }

    pub fn ansi_colors(mut self, enabled: bool) -> Self {
        self.config.ansi_colors = enabled;
        self
    }

    pub fn module_level(mut self, module: impl Into<String>, level: impl Into<String>) -> Self {
        self.config.module_levels.push(ModuleLevel {
            module: module.into(),
            level: level.into(),
        });
        self
    }

    pub fn span_events(mut self, new: bool, close: bool, enter: bool, exit: bool) -> Self {
        self.config.span_events = SpanEventConfig {
            new,
            close,
            enter,
            exit,
        };
        self
    }

    pub fn build(self) -> LogConfig {
        self.config
    }
}

impl Default for LogConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize logging with the given configuration
pub fn init_logging(config: &LogConfig) -> Result<(), LoggingError> {
    let filter = config.build_filter()?;
    let span_events = config.span_events.to_fmt_span();

    match (&config.format, &config.target) {
        (LogFormat::Json, LogTarget::Stdout) => {
            let layer = fmt::layer()
                .json()
                .with_timer(UtcTime::rfc_3339())
                .with_ansi(false)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stdout);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (LogFormat::Json, LogTarget::Stderr) => {
            let layer = fmt::layer()
                .json()
                .with_timer(UtcTime::rfc_3339())
                .with_ansi(false)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stderr);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (LogFormat::Pretty, LogTarget::Stdout) => {
            let layer = fmt::layer()
                .pretty()
                .with_ansi(config.ansi_colors)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stdout);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (LogFormat::Pretty, LogTarget::Stderr) => {
            let layer = fmt::layer()
                .pretty()
                .with_ansi(config.ansi_colors)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stderr);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (LogFormat::Compact, LogTarget::Stdout) => {
            let layer = fmt::layer()
                .compact()
                .with_ansi(config.ansi_colors)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stdout);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (LogFormat::Compact, LogTarget::Stderr) => {
            let layer = fmt::layer()
                .compact()
                .with_ansi(config.ansi_colors)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stderr);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (LogFormat::Full, LogTarget::Stdout) => {
            let layer = fmt::layer()
                .with_ansi(config.ansi_colors)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stdout);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (LogFormat::Full, LogTarget::Stderr) => {
            let layer = fmt::layer()
                .with_ansi(config.ansi_colors)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(io::stderr);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
        (_, LogTarget::File { path }) => {
            // File logging - create parent directories if needed
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;

            let layer = fmt::layer()
                .json()
                .with_timer(UtcTime::rfc_3339())
                .with_ansi(false)
                .with_file(config.file_line)
                .with_line_number(config.file_line)
                .with_thread_names(config.thread_names)
                .with_thread_ids(config.thread_ids)
                .with_target(config.target_module)
                .with_span_events(span_events)
                .with_writer(file);

            tracing_subscriber::registry()
                .with(layer.with_filter(filter))
                .try_init()
                .map_err(|e| LoggingError::SubscriberError(e.to_string()))?;
        }
    }

    Ok(())
}

/// Parse a string to a tracing Level
pub fn parse_level(level: &str) -> Result<Level, LoggingError> {
    match level.to_lowercase().as_str() {
        "trace" => Ok(Level::TRACE),
        "debug" => Ok(Level::DEBUG),
        "info" => Ok(Level::INFO),
        "warn" | "warning" => Ok(Level::WARN),
        "error" => Ok(Level::ERROR),
        _ => Err(LoggingError::InvalidLevel(level.to_string())),
    }
}

/// Parse a string to a LevelFilter
pub fn parse_level_filter(level: &str) -> Result<LevelFilter, LoggingError> {
    match level.to_lowercase().as_str() {
        "trace" => Ok(LevelFilter::TRACE),
        "debug" => Ok(LevelFilter::DEBUG),
        "info" => Ok(LevelFilter::INFO),
        "warn" | "warning" => Ok(LevelFilter::WARN),
        "error" => Ok(LevelFilter::ERROR),
        "off" => Ok(LevelFilter::OFF),
        _ => Err(LoggingError::InvalidLevel(level.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = LogConfig::default();
        assert_eq!(config.level, "info");
        assert!(matches!(config.format, LogFormat::Pretty));
        assert!(config.timestamps);
        assert!(config.ansi_colors);
    }

    #[test]
    fn test_development_config() {
        let config = LogConfig::development();
        assert_eq!(config.level, "debug");
        assert!(matches!(config.format, LogFormat::Pretty));
        assert!(config.file_line);
    }

    #[test]
    fn test_production_config() {
        let config = LogConfig::production();
        assert_eq!(config.level, "info");
        assert!(matches!(config.format, LogFormat::Json));
        assert!(!config.ansi_colors);
    }

    #[test]
    fn test_builder() {
        let config = LogConfigBuilder::new()
            .level("debug")
            .format(LogFormat::Json)
            .timestamps(true)
            .file_line(true)
            .module_level("scrapix_crawler", "trace")
            .build();

        assert_eq!(config.level, "debug");
        assert!(matches!(config.format, LogFormat::Json));
        assert!(config.file_line);
        assert_eq!(config.module_levels.len(), 1);
    }

    #[test]
    fn test_parse_level() {
        assert_eq!(parse_level("trace").unwrap(), Level::TRACE);
        assert_eq!(parse_level("debug").unwrap(), Level::DEBUG);
        assert_eq!(parse_level("info").unwrap(), Level::INFO);
        assert_eq!(parse_level("warn").unwrap(), Level::WARN);
        assert_eq!(parse_level("error").unwrap(), Level::ERROR);
        assert!(parse_level("invalid").is_err());
    }

    #[test]
    fn test_parse_level_filter() {
        assert_eq!(parse_level_filter("trace").unwrap(), LevelFilter::TRACE);
        assert_eq!(parse_level_filter("off").unwrap(), LevelFilter::OFF);
    }

    #[test]
    fn test_span_events_config() {
        let config = SpanEventConfig {
            new: true,
            close: true,
            enter: false,
            exit: false,
        };
        let span = config.to_fmt_span();
        // Verify the span events are set by checking it's not NONE
        assert_ne!(span, FmtSpan::NONE);
    }
}
