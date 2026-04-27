use std::env;

use url::Url;

use crate::error::HyperlineError;

pub const DEFAULT_API_BASE: &str = "https://api.hyperline.co";
pub const DEFAULT_INGEST_BASE: &str = "https://ingest.hyperline.co";
pub const SANDBOX_API_BASE: &str = "https://sandbox.api.hyperline.co";
pub const SANDBOX_INGEST_BASE: &str = "https://sandbox.ingest.hyperline.co";

#[derive(Debug, Clone)]
pub struct HyperlineConfig {
    pub api_key: String,
    pub api_base: Url,
    pub ingest_base: Url,
    pub webhook_secret: Option<String>,
}

impl HyperlineConfig {
    /// Load configuration from environment variables:
    /// - `HYPERLINE_API_KEY` (required) — prefixed `test_` or `prod_`.
    /// - `HYPERLINE_API_BASE` (optional) — defaults to sandbox for `test_` keys,
    ///   production for `prod_` keys.
    /// - `HYPERLINE_INGEST_BASE` (optional) — same default rule.
    /// - `HYPERLINE_WEBHOOK_SECRET` (optional) — required to verify webhooks.
    pub fn from_env() -> Result<Self, HyperlineError> {
        let api_key = env::var("HYPERLINE_API_KEY")
            .map_err(|_| HyperlineError::MissingEnv("HYPERLINE_API_KEY"))?;

        let is_sandbox = api_key.starts_with("test_");
        let default_api = if is_sandbox {
            SANDBOX_API_BASE
        } else {
            DEFAULT_API_BASE
        };
        let default_ingest = if is_sandbox {
            SANDBOX_INGEST_BASE
        } else {
            DEFAULT_INGEST_BASE
        };

        let api_base = env::var("HYPERLINE_API_BASE").unwrap_or_else(|_| default_api.into());
        let ingest_base =
            env::var("HYPERLINE_INGEST_BASE").unwrap_or_else(|_| default_ingest.into());

        Ok(Self {
            api_key,
            api_base: Url::parse(&api_base)?,
            ingest_base: Url::parse(&ingest_base)?,
            webhook_secret: env::var("HYPERLINE_WEBHOOK_SECRET").ok(),
        })
    }

    pub fn is_sandbox(&self) -> bool {
        self.api_key.starts_with("test_")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox_defaults_from_test_prefix() {
        // Guard against a developer having the var set in their shell.
        let prev = env::var("HYPERLINE_API_KEY").ok();
        let prev_base = env::var("HYPERLINE_API_BASE").ok();

        // SAFETY: tests may mutate process env; we restore at the end.
        unsafe {
            env::set_var("HYPERLINE_API_KEY", "test_abc");
            env::remove_var("HYPERLINE_API_BASE");
            env::remove_var("HYPERLINE_INGEST_BASE");
        }

        let cfg = HyperlineConfig::from_env().unwrap();
        assert!(cfg.is_sandbox());
        assert_eq!(cfg.api_base.as_str(), "https://sandbox.api.hyperline.co/");
        assert_eq!(
            cfg.ingest_base.as_str(),
            "https://sandbox.ingest.hyperline.co/"
        );

        // Restore.
        unsafe {
            match prev {
                Some(v) => env::set_var("HYPERLINE_API_KEY", v),
                None => env::remove_var("HYPERLINE_API_KEY"),
            }
            if let Some(v) = prev_base {
                env::set_var("HYPERLINE_API_BASE", v);
            }
        }
    }
}
