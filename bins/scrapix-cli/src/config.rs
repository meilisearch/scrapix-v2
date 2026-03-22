use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct CliConfig {
    #[serde(default)]
    pub api_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub output: Option<String>,

    // OAuth tokens (from `scrapix login`)
    #[serde(default)]
    pub access_token: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub token_expires_at: Option<i64>,
    #[serde(default)]
    pub oauth_client_id: Option<String>,
}

impl CliConfig {
    pub fn config_dir() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("Could not determine config directory")?
            .join("scrapix");
        Ok(dir)
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        toml::from_str(&content).with_context(|| "Failed to parse config.toml")
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    pub fn clear() -> Result<()> {
        let path = Self::config_path()?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Returns true if we have a valid (non-expired) access token
    pub fn has_valid_token(&self) -> bool {
        if self.access_token.is_none() {
            return false;
        }
        if let Some(expires_at) = self.token_expires_at {
            let now = chrono::Utc::now().timestamp();
            // Consider expired 60s before actual expiry for safety
            expires_at > now + 60
        } else {
            // No expiry info — assume valid
            true
        }
    }

    /// Resolve the best auth credential: API key takes priority, then Bearer token
    pub fn auth_credential(&self) -> Option<AuthCredential> {
        if let Some(ref key) = self.api_key {
            Some(AuthCredential::ApiKey(key.clone()))
        } else {
            self.access_token
                .as_ref()
                .map(|token| AuthCredential::Bearer(token.clone()))
        }
    }
}

#[derive(Debug, Clone)]
pub enum AuthCredential {
    ApiKey(String),
    Bearer(String),
}
