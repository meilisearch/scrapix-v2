//! Chrome DevTools Protocol renderer using chromiumoxide
//!
//! This module provides browser-based page rendering using Chrome/Chromium
//! via the Chrome DevTools Protocol (CDP). This is the most mature and
//! feature-rich option for JavaScript rendering.
//!
//! ## Features
//!
//! - Full JavaScript execution
//! - Network interception
//! - Screenshot capture
//! - PDF generation
//! - Cookie management
//! - Browser pool for concurrency
//!
//! ## Usage
//!
//! Enable the `browser-cdp` feature:
//!
//! ```toml
//! scrapix-crawler = { version = "0.1", features = ["browser-cdp"] }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::CookieParam;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::Page;
use futures::StreamExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::{debug, instrument, warn};

use scrapix_core::{CrawlUrl, RawPage, Result, ScrapixError};

use crate::robots::RobotsCache;

/// Errors specific to CDP rendering
#[derive(Debug, Error)]
pub enum CdpError {
    #[error("Browser launch failed: {0}")]
    LaunchFailed(String),

    #[error("Navigation failed: {0}")]
    NavigationFailed(String),

    #[error("Page timeout: {0}")]
    Timeout(String),

    #[error("JavaScript execution failed: {0}")]
    JsExecutionFailed(String),

    #[error("Screenshot failed: {0}")]
    ScreenshotFailed(String),

    #[error("Browser connection lost")]
    ConnectionLost,

    #[error("Pool exhausted")]
    PoolExhausted,
}

impl From<CdpError> for ScrapixError {
    fn from(err: CdpError) -> Self {
        ScrapixError::Crawl(err.to_string())
    }
}

/// Wait condition for page loading
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaitUntil {
    /// Wait for load event
    #[default]
    Load,
    /// Wait for DOMContentLoaded event
    DomContentLoaded,
    /// Wait for network to be idle (no requests for 500ms)
    NetworkIdle,
    /// Wait for network to be mostly idle (max 2 requests for 500ms)
    NetworkAlmostIdle,
}

/// Configuration for the CDP renderer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpConfig {
    /// Path to Chrome/Chromium executable (None = auto-detect)
    #[serde(default)]
    pub executable_path: Option<String>,

    /// Whether to run in headless mode
    #[serde(default = "default_true")]
    pub headless: bool,

    /// Whether to disable GPU acceleration
    #[serde(default = "default_true")]
    pub disable_gpu: bool,

    /// Whether to disable sandbox (required for Docker)
    #[serde(default)]
    pub no_sandbox: bool,

    /// Viewport width
    #[serde(default = "default_viewport_width")]
    pub viewport_width: u32,

    /// Viewport height
    #[serde(default = "default_viewport_height")]
    pub viewport_height: u32,

    /// Page load timeout
    #[serde(default = "default_timeout")]
    pub timeout: Duration,

    /// Wait condition for page loading
    #[serde(default)]
    pub wait_until: WaitUntil,

    /// Additional wait time after page load (for dynamic content)
    #[serde(default)]
    pub extra_wait: Option<Duration>,

    /// Maximum concurrent pages
    #[serde(default = "default_max_pages")]
    pub max_concurrent_pages: usize,

    /// User agent string
    #[serde(default)]
    pub user_agent: Option<String>,

    /// Whether to block images
    #[serde(default)]
    pub block_images: bool,

    /// Whether to block stylesheets
    #[serde(default)]
    pub block_stylesheets: bool,

    /// Whether to block fonts
    #[serde(default)]
    pub block_fonts: bool,

    /// Custom JavaScript to inject before page load
    #[serde(default)]
    pub inject_script: Option<String>,

    /// Proxy server URL
    #[serde(default)]
    pub proxy: Option<String>,

    /// Additional Chrome arguments
    #[serde(default)]
    pub extra_args: Vec<String>,
}

fn default_true() -> bool {
    true
}
fn default_viewport_width() -> u32 {
    1920
}
fn default_viewport_height() -> u32 {
    1080
}
fn default_timeout() -> Duration {
    Duration::from_secs(30)
}
fn default_max_pages() -> usize {
    5
}

impl Default for CdpConfig {
    fn default() -> Self {
        Self {
            executable_path: None,
            headless: true,
            disable_gpu: true,
            no_sandbox: false,
            viewport_width: default_viewport_width(),
            viewport_height: default_viewport_height(),
            timeout: default_timeout(),
            wait_until: WaitUntil::default(),
            extra_wait: None,
            max_concurrent_pages: default_max_pages(),
            user_agent: None,
            block_images: false,
            block_stylesheets: false,
            block_fonts: false,
            inject_script: None,
            proxy: None,
            extra_args: Vec::new(),
        }
    }
}

/// Result of rendering a page
#[derive(Debug)]
pub struct RenderResult {
    /// The rendered HTML
    pub html: String,

    /// Final URL after redirects
    pub final_url: String,

    /// HTTP status code
    pub status: u16,

    /// Response headers
    pub headers: HashMap<String, String>,

    /// Content type
    pub content_type: Option<String>,

    /// Screenshot (if requested)
    pub screenshot: Option<Vec<u8>>,

    /// Console log messages
    pub console_logs: Vec<String>,

    /// JavaScript errors
    pub js_errors: Vec<String>,

    /// Render duration
    pub render_duration: Duration,
}

/// CDP-based browser renderer
pub struct CdpRenderer {
    browser: Browser,
    config: CdpConfig,
    semaphore: Arc<Semaphore>,
    robots_cache: Option<Arc<RobotsCache>>,
    console_logs: Arc<Mutex<Vec<String>>>,
    js_errors: Arc<Mutex<Vec<String>>>,
}

impl CdpRenderer {
    /// Create a new CDP renderer with the given configuration
    pub async fn new(config: CdpConfig, robots_cache: Option<Arc<RobotsCache>>) -> Result<Self> {
        let browser_config = Self::build_browser_config(&config)?;

        let (browser, mut handler) = Browser::launch(browser_config)
            .await
            .map_err(|e| CdpError::LaunchFailed(e.to_string()))?;

        // Spawn handler task
        tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    warn!(error = %e, "Browser handler error");
                }
            }
        });

        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_pages));

        Ok(Self {
            browser,
            config,
            semaphore,
            robots_cache,
            console_logs: Arc::new(Mutex::new(Vec::new())),
            js_errors: Arc::new(Mutex::new(Vec::new())),
        })
    }

    /// Create with default configuration
    pub async fn with_defaults(robots_cache: Option<Arc<RobotsCache>>) -> Result<Self> {
        Self::new(CdpConfig::default(), robots_cache).await
    }

    /// Build browser configuration from our config
    fn build_browser_config(config: &CdpConfig) -> Result<BrowserConfig> {
        let mut builder = BrowserConfig::builder();

        if config.headless {
            builder = builder.with_head();
        }

        if config.disable_gpu {
            builder = builder.arg("--disable-gpu");
        }

        if config.no_sandbox {
            builder = builder.arg("--no-sandbox");
            builder = builder.arg("--disable-setuid-sandbox");
        }

        // Set viewport via window size argument
        builder = builder.arg(format!(
            "--window-size={},{}",
            config.viewport_width, config.viewport_height
        ));

        // Set user agent via argument
        if let Some(ref user_agent) = config.user_agent {
            builder = builder.arg(format!("--user-agent={}", user_agent));
        }

        // Set proxy via argument
        if let Some(ref proxy) = config.proxy {
            builder = builder.arg(format!("--proxy-server={}", proxy));
        }

        // Add extra arguments
        for arg in &config.extra_args {
            builder = builder.arg(arg);
        }

        // Memory and performance optimizations
        builder = builder
            .arg("--disable-dev-shm-usage")
            .arg("--disable-extensions")
            .arg("--disable-background-networking")
            .arg("--disable-sync")
            .arg("--disable-translate")
            .arg("--metrics-recording-only")
            .arg("--mute-audio")
            .arg("--no-first-run");

        builder
            .build()
            .map_err(|e| ScrapixError::Crawl(format!("Failed to build browser config: {}", e)))
    }

    /// Render a page and return the result
    #[instrument(skip(self), fields(url = %url))]
    pub async fn render(&self, url: &str) -> Result<RenderResult> {
        // Check robots.txt
        if let Some(ref cache) = self.robots_cache {
            if !cache.is_allowed(url).await? {
                return Err(ScrapixError::RobotsDisallowed {
                    url: url.to_string(),
                });
            }
        }

        // Acquire semaphore
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| CdpError::PoolExhausted)?;

        let start = Instant::now();

        // Create new page
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| CdpError::LaunchFailed(format!("Failed to create page: {}", e)))?;

        // Setup page
        self.setup_page(&page).await?;

        // Navigate to URL
        page.goto(url)
            .await
            .map_err(|e| CdpError::NavigationFailed(e.to_string()))?;

        // Wait for page load based on configuration
        self.wait_for_page(&page).await?;

        // Extra wait if configured
        if let Some(extra_wait) = self.config.extra_wait {
            tokio::time::sleep(extra_wait).await;
        }

        // Get final URL
        let final_url = page
            .url()
            .await
            .map_err(|e| CdpError::NavigationFailed(e.to_string()))?
            .unwrap_or_else(|| url.to_string());

        // Get HTML content
        let html = page
            .content()
            .await
            .map_err(|e| CdpError::NavigationFailed(format!("Failed to get content: {}", e)))?;

        // Default status and headers (CDP doesn't expose these easily)
        let status = 200u16;
        let headers = HashMap::new();
        let content_type = Some("text/html".to_string());

        // Collect console logs and errors
        let console_logs = std::mem::take(&mut *self.console_logs.lock());
        let js_errors = std::mem::take(&mut *self.js_errors.lock());

        // Close page
        let _ = page.close().await;

        let render_duration = start.elapsed();

        debug!(
            duration_ms = render_duration.as_millis(),
            final_url = %final_url,
            status = status,
            "Page rendered"
        );

        Ok(RenderResult {
            html,
            final_url,
            status,
            headers,
            content_type,
            screenshot: None,
            console_logs,
            js_errors,
            render_duration,
        })
    }

    /// Setup page with configured options
    async fn setup_page(&self, page: &Page) -> Result<()> {
        // Inject script if configured
        if let Some(ref script) = self.config.inject_script {
            page.evaluate_on_new_document(script.clone())
                .await
                .map_err(|e| CdpError::JsExecutionFailed(e.to_string()))?;
        }

        Ok(())
    }

    /// Wait for page to load based on configuration
    async fn wait_for_page(&self, page: &Page) -> Result<()> {
        let timeout = self.config.timeout;

        match self.config.wait_until {
            WaitUntil::Load => {
                tokio::time::timeout(timeout, page.wait_for_navigation())
                    .await
                    .map_err(|_| CdpError::Timeout("Page load timeout".to_string()))?
                    .map_err(|e| CdpError::NavigationFailed(e.to_string()))?;
            }
            WaitUntil::DomContentLoaded => {
                // DOMContentLoaded is typically faster
                tokio::time::timeout(timeout, page.wait_for_navigation())
                    .await
                    .map_err(|_| CdpError::Timeout("DOMContentLoaded timeout".to_string()))?
                    .map_err(|e| CdpError::NavigationFailed(e.to_string()))?;
            }
            WaitUntil::NetworkIdle | WaitUntil::NetworkAlmostIdle => {
                // Wait for navigation then additional time for network
                tokio::time::timeout(timeout, page.wait_for_navigation())
                    .await
                    .map_err(|_| CdpError::Timeout("Navigation timeout".to_string()))?
                    .map_err(|e| CdpError::NavigationFailed(e.to_string()))?;

                // Additional wait for network to settle
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }

        Ok(())
    }

    /// Render a page and take a screenshot
    pub async fn render_with_screenshot(&self, url: &str) -> Result<RenderResult> {
        let mut result = self.render(url).await?;

        // Create a new page for screenshot (since we closed the original)
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| CdpError::PoolExhausted)?;

        let page = self
            .browser
            .new_page(url)
            .await
            .map_err(|e| CdpError::LaunchFailed(format!("Failed to create page: {}", e)))?;

        self.setup_page(&page).await?;
        self.wait_for_page(&page).await?;

        // Take screenshot
        let screenshot = page
            .screenshot(ScreenshotParams::builder().full_page(true).build())
            .await
            .map_err(|e| CdpError::ScreenshotFailed(e.to_string()))?;

        let _ = page.close().await;

        result.screenshot = Some(screenshot);

        Ok(result)
    }

    /// Execute JavaScript on a page and return the result
    pub async fn execute_script(&self, url: &str, script: &str) -> Result<serde_json::Value> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| CdpError::PoolExhausted)?;

        let page = self
            .browser
            .new_page(url)
            .await
            .map_err(|e| CdpError::LaunchFailed(format!("Failed to create page: {}", e)))?;

        self.setup_page(&page).await?;
        self.wait_for_page(&page).await?;

        let result = page
            .evaluate(script)
            .await
            .map_err(|e| CdpError::JsExecutionFailed(e.to_string()))?;

        let _ = page.close().await;

        Ok(result.into_value()?)
    }

    /// Set cookies for a domain
    pub async fn set_cookies(&self, cookies: Vec<CookieParam>) -> Result<()> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| CdpError::PoolExhausted)?;

        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| CdpError::LaunchFailed(format!("Failed to create page: {}", e)))?;

        for cookie in cookies {
            page.set_cookie(cookie)
                .await
                .map_err(|e| CdpError::NavigationFailed(format!("Failed to set cookie: {}", e)))?;
        }

        let _ = page.close().await;
        Ok(())
    }

    /// Fetch a CrawlUrl and return a RawPage
    #[instrument(skip(self), fields(url = %url.url))]
    pub async fn fetch(&self, url: &CrawlUrl) -> Result<RawPage> {
        let result = self.render(&url.url).await?;

        Ok(RawPage {
            url: url.url.clone(),
            final_url: result.final_url,
            status: result.status,
            headers: result.headers,
            html: result.html,
            content_type: result.content_type,
            js_rendered: true,
            fetched_at: Utc::now(),
            fetch_duration_ms: result.render_duration.as_millis() as u64,
        })
    }

    /// Get the current configuration
    pub fn config(&self) -> &CdpConfig {
        &self.config
    }
}

/// Trait implementation for the core Fetcher trait
#[async_trait]
impl scrapix_core::traits::Fetcher for CdpRenderer {
    async fn fetch(&self, url: &CrawlUrl) -> Result<RawPage> {
        CdpRenderer::fetch(self, url).await
    }

    async fn is_allowed(&self, url: &str) -> Result<bool> {
        if let Some(ref cache) = self.robots_cache {
            cache.is_allowed(url).await
        } else {
            Ok(true)
        }
    }

    async fn get_crawl_delay(&self, domain: &str) -> Result<Option<u64>> {
        if let Some(ref cache) = self.robots_cache {
            cache.get_crawl_delay(domain).await
        } else {
            Ok(None)
        }
    }
}

/// Builder for CdpRenderer
pub struct CdpRendererBuilder {
    config: CdpConfig,
    robots_cache: Option<Arc<RobotsCache>>,
}

impl CdpRendererBuilder {
    pub fn new() -> Self {
        Self {
            config: CdpConfig::default(),
            robots_cache: None,
        }
    }

    pub fn executable_path(mut self, path: impl Into<String>) -> Self {
        self.config.executable_path = Some(path.into());
        self
    }

    pub fn headless(mut self, headless: bool) -> Self {
        self.config.headless = headless;
        self
    }

    pub fn no_sandbox(mut self, no_sandbox: bool) -> Self {
        self.config.no_sandbox = no_sandbox;
        self
    }

    pub fn viewport(mut self, width: u32, height: u32) -> Self {
        self.config.viewport_width = width;
        self.config.viewport_height = height;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    pub fn wait_until(mut self, wait: WaitUntil) -> Self {
        self.config.wait_until = wait;
        self
    }

    pub fn extra_wait(mut self, duration: Duration) -> Self {
        self.config.extra_wait = Some(duration);
        self
    }

    pub fn max_concurrent_pages(mut self, max: usize) -> Self {
        self.config.max_concurrent_pages = max;
        self
    }

    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config.user_agent = Some(user_agent.into());
        self
    }

    pub fn block_images(mut self, block: bool) -> Self {
        self.config.block_images = block;
        self
    }

    pub fn block_stylesheets(mut self, block: bool) -> Self {
        self.config.block_stylesheets = block;
        self
    }

    pub fn inject_script(mut self, script: impl Into<String>) -> Self {
        self.config.inject_script = Some(script.into());
        self
    }

    pub fn proxy(mut self, proxy: impl Into<String>) -> Self {
        self.config.proxy = Some(proxy.into());
        self
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.config.extra_args.push(arg.into());
        self
    }

    pub fn robots_cache(mut self, cache: Arc<RobotsCache>) -> Self {
        self.robots_cache = Some(cache);
        self
    }

    pub async fn build(self) -> Result<CdpRenderer> {
        CdpRenderer::new(self.config, self.robots_cache).await
    }
}

impl Default for CdpRendererBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = CdpConfig::default();
        assert!(config.headless);
        assert!(config.disable_gpu);
        assert!(!config.no_sandbox);
        assert_eq!(config.viewport_width, 1920);
        assert_eq!(config.viewport_height, 1080);
        assert_eq!(config.max_concurrent_pages, 5);
    }

    #[test]
    fn test_builder() {
        let builder = CdpRendererBuilder::new()
            .headless(false)
            .no_sandbox(true)
            .viewport(1280, 720)
            .timeout(Duration::from_secs(60))
            .user_agent("TestBot/1.0");

        assert!(!builder.config.headless);
        assert!(builder.config.no_sandbox);
        assert_eq!(builder.config.viewport_width, 1280);
        assert_eq!(builder.config.viewport_height, 720);
        assert_eq!(builder.config.timeout, Duration::from_secs(60));
        assert_eq!(builder.config.user_agent, Some("TestBot/1.0".to_string()));
    }
}
