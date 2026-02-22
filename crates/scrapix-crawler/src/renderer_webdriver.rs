//! WebDriver protocol renderer using fantoccini
//!
//! This module provides browser-based page rendering using the WebDriver protocol,
//! which supports multiple browsers including Firefox, Chrome, Safari, and Edge.
//!
//! ## Features
//!
//! - Cross-browser support (Firefox, Chrome, Safari, Edge)
//! - Full JavaScript execution
//! - Screenshot capture
//! - Cookie management
//! - Form interaction
//! - Concurrent page rendering
//!
//! ## Usage
//!
//! Enable the `browser-webdriver` feature:
//!
//! ```toml
//! scrapix-crawler = { version = "0.1", features = ["browser-webdriver"] }
//! ```
//!
//! ## WebDriver Server
//!
//! You need a running WebDriver server:
//! - Firefox: `geckodriver`
//! - Chrome: `chromedriver`
//! - Safari: `safaridriver`
//! - Edge: `msedgedriver`

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use fantoccini::wd::Capabilities;
use fantoccini::{Client, ClientBuilder, Locator};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::{debug, instrument, warn};

use scrapix_core::{CrawlUrl, RawPage, Result, ScrapixError};

use crate::robots::RobotsCache;

/// Errors specific to WebDriver rendering
#[derive(Debug, Error)]
pub enum WebDriverError {
    #[error("WebDriver connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Session creation failed: {0}")]
    SessionFailed(String),

    #[error("Navigation failed: {0}")]
    NavigationFailed(String),

    #[error("Page timeout: {0}")]
    Timeout(String),

    #[error("JavaScript execution failed: {0}")]
    JsExecutionFailed(String),

    #[error("Screenshot failed: {0}")]
    ScreenshotFailed(String),

    #[error("Element not found: {0}")]
    ElementNotFound(String),

    #[error("Pool exhausted")]
    PoolExhausted,

    #[error("Browser not supported: {0}")]
    UnsupportedBrowser(String),
}

impl From<WebDriverError> for ScrapixError {
    fn from(err: WebDriverError) -> Self {
        ScrapixError::Crawl(err.to_string())
    }
}

impl From<fantoccini::error::CmdError> for WebDriverError {
    fn from(err: fantoccini::error::CmdError) -> Self {
        WebDriverError::NavigationFailed(err.to_string())
    }
}

impl From<fantoccini::error::NewSessionError> for WebDriverError {
    fn from(err: fantoccini::error::NewSessionError) -> Self {
        WebDriverError::SessionFailed(err.to_string())
    }
}

/// Supported browser types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BrowserType {
    /// Mozilla Firefox (requires geckodriver)
    #[default]
    Firefox,
    /// Google Chrome (requires chromedriver)
    Chrome,
    /// Apple Safari (requires safaridriver, macOS only)
    Safari,
    /// Microsoft Edge (requires msedgedriver)
    Edge,
}

impl BrowserType {
    /// Get the WebDriver capability name for this browser
    pub fn capability_name(&self) -> &'static str {
        match self {
            BrowserType::Firefox => "moz:firefoxOptions",
            BrowserType::Chrome => "goog:chromeOptions",
            BrowserType::Safari => "safari:options",
            BrowserType::Edge => "ms:edgeOptions",
        }
    }

    /// Get the browser name for capabilities
    pub fn browser_name(&self) -> &'static str {
        match self {
            BrowserType::Firefox => "firefox",
            BrowserType::Chrome => "chrome",
            BrowserType::Safari => "safari",
            BrowserType::Edge => "MicrosoftEdge",
        }
    }
}

/// Wait strategy for page loading
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PageLoadStrategy {
    /// Wait for full page load (default)
    #[default]
    Normal,
    /// Wait only for initial page load (DOMContentLoaded)
    Eager,
    /// Don't wait for page load
    None,
}

impl PageLoadStrategy {
    fn as_str(&self) -> &'static str {
        match self {
            PageLoadStrategy::Normal => "normal",
            PageLoadStrategy::Eager => "eager",
            PageLoadStrategy::None => "none",
        }
    }
}

/// Configuration for the WebDriver renderer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDriverConfig {
    /// WebDriver server URL
    #[serde(default = "default_webdriver_url")]
    pub webdriver_url: String,

    /// Browser type to use
    #[serde(default)]
    pub browser: BrowserType,

    /// Whether to run in headless mode
    #[serde(default = "default_true")]
    pub headless: bool,

    /// Viewport width
    #[serde(default = "default_viewport_width")]
    pub viewport_width: u32,

    /// Viewport height
    #[serde(default = "default_viewport_height")]
    pub viewport_height: u32,

    /// Page load timeout
    #[serde(default = "default_timeout")]
    pub timeout: Duration,

    /// Page load strategy
    #[serde(default)]
    pub page_load_strategy: PageLoadStrategy,

    /// Additional wait time after page load (for dynamic content)
    #[serde(default)]
    pub extra_wait: Option<Duration>,

    /// Maximum concurrent sessions
    #[serde(default = "default_max_sessions")]
    pub max_concurrent_sessions: usize,

    /// User agent string
    #[serde(default)]
    pub user_agent: Option<String>,

    /// Accept insecure certificates
    #[serde(default)]
    pub accept_insecure_certs: bool,

    /// Custom JavaScript to inject after page load
    #[serde(default)]
    pub inject_script: Option<String>,

    /// Proxy server URL
    #[serde(default)]
    pub proxy: Option<String>,

    /// Additional browser arguments
    #[serde(default)]
    pub browser_args: Vec<String>,
}

fn default_webdriver_url() -> String {
    "http://localhost:4444".to_string()
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
fn default_max_sessions() -> usize {
    5
}

impl Default for WebDriverConfig {
    fn default() -> Self {
        Self {
            webdriver_url: default_webdriver_url(),
            browser: BrowserType::default(),
            headless: true,
            viewport_width: default_viewport_width(),
            viewport_height: default_viewport_height(),
            timeout: default_timeout(),
            page_load_strategy: PageLoadStrategy::default(),
            extra_wait: None,
            max_concurrent_sessions: default_max_sessions(),
            user_agent: None,
            accept_insecure_certs: false,
            inject_script: None,
            proxy: None,
            browser_args: Vec::new(),
        }
    }
}

/// Result of rendering a page via WebDriver
#[derive(Debug)]
pub struct WebDriverRenderResult {
    /// The rendered HTML
    pub html: String,

    /// Final URL after redirects
    pub final_url: String,

    /// Page title
    pub title: Option<String>,

    /// Screenshot (if requested)
    pub screenshot: Option<Vec<u8>>,

    /// Render duration
    pub render_duration: Duration,
}

/// WebDriver-based browser renderer
pub struct WebDriverRenderer {
    config: WebDriverConfig,
    semaphore: Arc<Semaphore>,
    robots_cache: Option<Arc<RobotsCache>>,
}

impl WebDriverRenderer {
    /// Create a new WebDriver renderer with the given configuration
    pub fn new(config: WebDriverConfig, robots_cache: Option<Arc<RobotsCache>>) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent_sessions));

        Self {
            config,
            semaphore,
            robots_cache,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(robots_cache: Option<Arc<RobotsCache>>) -> Self {
        Self::new(WebDriverConfig::default(), robots_cache)
    }

    /// Build capabilities based on configuration
    fn build_capabilities(&self) -> Capabilities {
        let mut caps = Capabilities::new();

        // Set browser name
        caps.insert(
            "browserName".to_string(),
            json!(self.config.browser.browser_name()),
        );

        // Set page load strategy
        caps.insert(
            "pageLoadStrategy".to_string(),
            json!(self.config.page_load_strategy.as_str()),
        );

        // Accept insecure certificates
        if self.config.accept_insecure_certs {
            caps.insert("acceptInsecureCerts".to_string(), json!(true));
        }

        // Set timeouts
        caps.insert(
            "timeouts".to_string(),
            json!({
                "pageLoad": self.config.timeout.as_millis() as u64,
                "script": self.config.timeout.as_millis() as u64,
            }),
        );

        // Browser-specific options
        match self.config.browser {
            BrowserType::Firefox => {
                let mut args = self.config.browser_args.clone();
                if self.config.headless {
                    args.push("-headless".to_string());
                }
                if let Some(ref ua) = self.config.user_agent {
                    // Firefox uses preferences for user agent
                    caps.insert(
                        "moz:firefoxOptions".to_string(),
                        json!({
                            "args": args,
                            "prefs": {
                                "general.useragent.override": ua
                            }
                        }),
                    );
                } else {
                    caps.insert(
                        "moz:firefoxOptions".to_string(),
                        json!({
                            "args": args
                        }),
                    );
                }
            }
            BrowserType::Chrome => {
                let mut args = self.config.browser_args.clone();
                if self.config.headless {
                    args.push("--headless=new".to_string());
                }
                args.push(format!(
                    "--window-size={},{}",
                    self.config.viewport_width, self.config.viewport_height
                ));
                if let Some(ref ua) = self.config.user_agent {
                    args.push(format!("--user-agent={}", ua));
                }
                if let Some(ref proxy) = self.config.proxy {
                    args.push(format!("--proxy-server={}", proxy));
                }
                // Common Chrome flags for stability
                args.push("--disable-gpu".to_string());
                args.push("--no-sandbox".to_string());
                args.push("--disable-dev-shm-usage".to_string());

                caps.insert(
                    "goog:chromeOptions".to_string(),
                    json!({
                        "args": args
                    }),
                );
            }
            BrowserType::Safari => {
                // Safari has limited options
                caps.insert(
                    "safari:options".to_string(),
                    json!({
                        "automaticInspection": false,
                        "automaticProfiling": false
                    }),
                );
            }
            BrowserType::Edge => {
                let mut args = self.config.browser_args.clone();
                if self.config.headless {
                    args.push("--headless=new".to_string());
                }
                args.push(format!(
                    "--window-size={},{}",
                    self.config.viewport_width, self.config.viewport_height
                ));
                if let Some(ref ua) = self.config.user_agent {
                    args.push(format!("--user-agent={}", ua));
                }

                caps.insert(
                    "ms:edgeOptions".to_string(),
                    json!({
                        "args": args
                    }),
                );
            }
        }

        caps
    }

    /// Create a new WebDriver client
    async fn create_client(&self) -> std::result::Result<Client, WebDriverError> {
        let caps = self.build_capabilities();

        ClientBuilder::native()
            .capabilities(caps)
            .connect(&self.config.webdriver_url)
            .await
            .map_err(|e| WebDriverError::ConnectionFailed(e.to_string()))
    }

    /// Render a page and return the result
    #[instrument(skip(self), fields(url = %url))]
    pub async fn render(&self, url: &str) -> Result<WebDriverRenderResult> {
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
            .map_err(|_| WebDriverError::PoolExhausted)?;

        let start = Instant::now();

        // Create client
        let client = self.create_client().await?;

        // Navigate to URL
        client
            .goto(url)
            .await
            .map_err(|e| WebDriverError::NavigationFailed(e.to_string()))?;

        // Extra wait if configured
        if let Some(extra_wait) = self.config.extra_wait {
            tokio::time::sleep(extra_wait).await;
        }

        // Inject script if configured
        if let Some(ref script) = self.config.inject_script {
            if let Err(e) = client.execute(script, vec![]).await {
                warn!(error = %e, "Failed to inject script");
            }
        }

        // Get final URL
        let final_url = client
            .current_url()
            .await
            .map(|u| u.to_string())
            .unwrap_or_else(|_| url.to_string());

        // Get page title
        let title = client.title().await.ok();

        // Get HTML content
        let html = client.source().await.map_err(|e| {
            WebDriverError::NavigationFailed(format!("Failed to get source: {}", e))
        })?;

        let render_duration = start.elapsed();

        // Close the session
        if let Err(e) = client.close().await {
            warn!(error = %e, "Failed to close WebDriver session");
        }

        debug!(
            duration_ms = render_duration.as_millis(),
            final_url = %final_url,
            "Page rendered via WebDriver"
        );

        Ok(WebDriverRenderResult {
            html,
            final_url,
            title,
            screenshot: None,
            render_duration,
        })
    }

    /// Render a page and take a screenshot
    pub async fn render_with_screenshot(&self, url: &str) -> Result<WebDriverRenderResult> {
        // Check robots.txt
        if let Some(ref cache) = self.robots_cache {
            if !cache.is_allowed(url).await? {
                return Err(ScrapixError::RobotsDisallowed {
                    url: url.to_string(),
                });
            }
        }

        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| WebDriverError::PoolExhausted)?;

        let start = Instant::now();

        let client = self.create_client().await?;

        client
            .goto(url)
            .await
            .map_err(|e| WebDriverError::NavigationFailed(e.to_string()))?;

        if let Some(extra_wait) = self.config.extra_wait {
            tokio::time::sleep(extra_wait).await;
        }

        if let Some(ref script) = self.config.inject_script {
            let _ = client.execute(script, vec![]).await;
        }

        let final_url = client
            .current_url()
            .await
            .map(|u| u.to_string())
            .unwrap_or_else(|_| url.to_string());

        let title = client.title().await.ok();

        let html = client.source().await.map_err(|e| {
            WebDriverError::NavigationFailed(format!("Failed to get source: {}", e))
        })?;

        // Take screenshot
        let screenshot = client
            .screenshot()
            .await
            .map_err(|e| WebDriverError::ScreenshotFailed(e.to_string()))?;

        let render_duration = start.elapsed();

        if let Err(e) = client.close().await {
            warn!(error = %e, "Failed to close WebDriver session");
        }

        Ok(WebDriverRenderResult {
            html,
            final_url,
            title,
            screenshot: Some(screenshot),
            render_duration,
        })
    }

    /// Execute JavaScript on a page and return the result
    pub async fn execute_script(&self, url: &str, script: &str) -> Result<serde_json::Value> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| WebDriverError::PoolExhausted)?;

        let client = self.create_client().await?;

        client
            .goto(url)
            .await
            .map_err(|e| WebDriverError::NavigationFailed(e.to_string()))?;

        if let Some(extra_wait) = self.config.extra_wait {
            tokio::time::sleep(extra_wait).await;
        }

        let result = client
            .execute(script, vec![])
            .await
            .map_err(|e| WebDriverError::JsExecutionFailed(e.to_string()))?;

        if let Err(e) = client.close().await {
            warn!(error = %e, "Failed to close WebDriver session");
        }

        Ok(result)
    }

    /// Find an element and get its text content
    pub async fn find_element_text(&self, url: &str, selector: &str) -> Result<String> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| WebDriverError::PoolExhausted)?;

        let client = self.create_client().await?;

        client
            .goto(url)
            .await
            .map_err(|e| WebDriverError::NavigationFailed(e.to_string()))?;

        if let Some(extra_wait) = self.config.extra_wait {
            tokio::time::sleep(extra_wait).await;
        }

        let element = client
            .find(Locator::Css(selector))
            .await
            .map_err(|e| WebDriverError::ElementNotFound(e.to_string()))?;

        let text = element
            .text()
            .await
            .map_err(|e| WebDriverError::ElementNotFound(format!("Failed to get text: {}", e)))?;

        if let Err(e) = client.close().await {
            warn!(error = %e, "Failed to close WebDriver session");
        }

        Ok(text)
    }

    /// Wait for an element to appear
    pub async fn wait_for_element(&self, url: &str, selector: &str) -> Result<bool> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| WebDriverError::PoolExhausted)?;

        let client = self.create_client().await?;

        client
            .goto(url)
            .await
            .map_err(|e| WebDriverError::NavigationFailed(e.to_string()))?;

        // Wait for element with timeout
        let wait_result = client.wait().for_element(Locator::Css(selector)).await;

        let found = wait_result.is_ok();

        if let Err(e) = client.close().await {
            warn!(error = %e, "Failed to close WebDriver session");
        }

        Ok(found)
    }

    /// Click on an element
    pub async fn click_element(&self, url: &str, selector: &str) -> Result<String> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| WebDriverError::PoolExhausted)?;

        let client = self.create_client().await?;

        client
            .goto(url)
            .await
            .map_err(|e| WebDriverError::NavigationFailed(e.to_string()))?;

        if let Some(extra_wait) = self.config.extra_wait {
            tokio::time::sleep(extra_wait).await;
        }

        let element = client
            .find(Locator::Css(selector))
            .await
            .map_err(|e| WebDriverError::ElementNotFound(e.to_string()))?;

        element
            .click()
            .await
            .map_err(|e| WebDriverError::ElementNotFound(format!("Failed to click: {}", e)))?;

        // Wait a bit for any navigation or JS execution
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Get the resulting page source
        let html = client.source().await.map_err(|e| {
            WebDriverError::NavigationFailed(format!("Failed to get source: {}", e))
        })?;

        if let Err(e) = client.close().await {
            warn!(error = %e, "Failed to close WebDriver session");
        }

        Ok(html)
    }

    /// Fill a form field
    pub async fn fill_field(&self, url: &str, selector: &str, value: &str) -> Result<()> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| WebDriverError::PoolExhausted)?;

        let client = self.create_client().await?;

        client
            .goto(url)
            .await
            .map_err(|e| WebDriverError::NavigationFailed(e.to_string()))?;

        if let Some(extra_wait) = self.config.extra_wait {
            tokio::time::sleep(extra_wait).await;
        }

        let element = client
            .find(Locator::Css(selector))
            .await
            .map_err(|e| WebDriverError::ElementNotFound(e.to_string()))?;

        element
            .send_keys(value)
            .await
            .map_err(|e| WebDriverError::ElementNotFound(format!("Failed to fill: {}", e)))?;

        if let Err(e) = client.close().await {
            warn!(error = %e, "Failed to close WebDriver session");
        }

        Ok(())
    }

    /// Fetch a CrawlUrl and return a RawPage
    #[instrument(skip(self), fields(url = %url.url))]
    pub async fn fetch(&self, url: &CrawlUrl) -> Result<RawPage> {
        let result = self.render(&url.url).await?;

        Ok(RawPage {
            url: url.url.clone(),
            final_url: result.final_url,
            status: 200,             // WebDriver doesn't expose HTTP status
            headers: HashMap::new(), // WebDriver doesn't expose headers
            html: result.html,
            content_type: Some("text/html".to_string()),
            js_rendered: true,
            fetched_at: Utc::now(),
            fetch_duration_ms: result.render_duration.as_millis() as u64,
        })
    }

    /// Get the current configuration
    pub fn config(&self) -> &WebDriverConfig {
        &self.config
    }
}

/// Trait implementation for the core Fetcher trait
#[async_trait]
impl scrapix_core::traits::Fetcher for WebDriverRenderer {
    async fn fetch(&self, url: &CrawlUrl) -> Result<RawPage> {
        WebDriverRenderer::fetch(self, url).await
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

/// Builder for WebDriverRenderer
pub struct WebDriverRendererBuilder {
    config: WebDriverConfig,
    robots_cache: Option<Arc<RobotsCache>>,
}

impl WebDriverRendererBuilder {
    pub fn new() -> Self {
        Self {
            config: WebDriverConfig::default(),
            robots_cache: None,
        }
    }

    /// Set the WebDriver server URL
    pub fn webdriver_url(mut self, url: impl Into<String>) -> Self {
        self.config.webdriver_url = url.into();
        self
    }

    /// Set the browser type
    pub fn browser(mut self, browser: BrowserType) -> Self {
        self.config.browser = browser;
        self
    }

    /// Use Firefox browser
    pub fn firefox(mut self) -> Self {
        self.config.browser = BrowserType::Firefox;
        self
    }

    /// Use Chrome browser
    pub fn chrome(mut self) -> Self {
        self.config.browser = BrowserType::Chrome;
        self
    }

    /// Use Safari browser
    pub fn safari(mut self) -> Self {
        self.config.browser = BrowserType::Safari;
        self
    }

    /// Use Edge browser
    pub fn edge(mut self) -> Self {
        self.config.browser = BrowserType::Edge;
        self
    }

    /// Set headless mode
    pub fn headless(mut self, headless: bool) -> Self {
        self.config.headless = headless;
        self
    }

    /// Set viewport dimensions
    pub fn viewport(mut self, width: u32, height: u32) -> Self {
        self.config.viewport_width = width;
        self.config.viewport_height = height;
        self
    }

    /// Set page load timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Set page load strategy
    pub fn page_load_strategy(mut self, strategy: PageLoadStrategy) -> Self {
        self.config.page_load_strategy = strategy;
        self
    }

    /// Set extra wait time after page load
    pub fn extra_wait(mut self, duration: Duration) -> Self {
        self.config.extra_wait = Some(duration);
        self
    }

    /// Set maximum concurrent sessions
    pub fn max_concurrent_sessions(mut self, max: usize) -> Self {
        self.config.max_concurrent_sessions = max;
        self
    }

    /// Set user agent
    pub fn user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.config.user_agent = Some(user_agent.into());
        self
    }

    /// Accept insecure certificates
    pub fn accept_insecure_certs(mut self, accept: bool) -> Self {
        self.config.accept_insecure_certs = accept;
        self
    }

    /// Set JavaScript to inject after page load
    pub fn inject_script(mut self, script: impl Into<String>) -> Self {
        self.config.inject_script = Some(script.into());
        self
    }

    /// Set proxy server
    pub fn proxy(mut self, proxy: impl Into<String>) -> Self {
        self.config.proxy = Some(proxy.into());
        self
    }

    /// Add a browser argument
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.config.browser_args.push(arg.into());
        self
    }

    /// Set robots cache
    pub fn robots_cache(mut self, cache: Arc<RobotsCache>) -> Self {
        self.robots_cache = Some(cache);
        self
    }

    /// Build the renderer
    pub fn build(self) -> WebDriverRenderer {
        WebDriverRenderer::new(self.config, self.robots_cache)
    }
}

impl Default for WebDriverRendererBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Cookie management for WebDriver sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDriverCookie {
    /// Cookie name
    pub name: String,
    /// Cookie value
    pub value: String,
    /// Cookie domain
    pub domain: Option<String>,
    /// Cookie path
    pub path: Option<String>,
    /// Whether the cookie is secure
    pub secure: Option<bool>,
    /// Whether the cookie is HTTP-only
    pub http_only: Option<bool>,
    /// Expiry timestamp (seconds since Unix epoch)
    pub expiry: Option<u64>,
}

impl WebDriverCookie {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            domain: None,
            path: None,
            secure: None,
            http_only: None,
            expiry: None,
        }
    }

    pub fn domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = Some(secure);
        self
    }

    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = Some(http_only);
        self
    }

    pub fn expiry(mut self, expiry: u64) -> Self {
        self.expiry = Some(expiry);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = WebDriverConfig::default();
        assert_eq!(config.webdriver_url, "http://localhost:4444");
        assert!(matches!(config.browser, BrowserType::Firefox));
        assert!(config.headless);
        assert_eq!(config.viewport_width, 1920);
        assert_eq!(config.viewport_height, 1080);
        assert_eq!(config.max_concurrent_sessions, 5);
    }

    #[test]
    fn test_browser_type_names() {
        assert_eq!(BrowserType::Firefox.browser_name(), "firefox");
        assert_eq!(BrowserType::Chrome.browser_name(), "chrome");
        assert_eq!(BrowserType::Safari.browser_name(), "safari");
        assert_eq!(BrowserType::Edge.browser_name(), "MicrosoftEdge");
    }

    #[test]
    fn test_page_load_strategy() {
        assert_eq!(PageLoadStrategy::Normal.as_str(), "normal");
        assert_eq!(PageLoadStrategy::Eager.as_str(), "eager");
        assert_eq!(PageLoadStrategy::None.as_str(), "none");
    }

    #[test]
    fn test_builder() {
        let renderer = WebDriverRendererBuilder::new()
            .webdriver_url("http://localhost:9515")
            .chrome()
            .headless(false)
            .viewport(1280, 720)
            .timeout(Duration::from_secs(60))
            .user_agent("TestBot/1.0")
            .extra_wait(Duration::from_secs(2))
            .build();

        assert_eq!(renderer.config.webdriver_url, "http://localhost:9515");
        assert!(matches!(renderer.config.browser, BrowserType::Chrome));
        assert!(!renderer.config.headless);
        assert_eq!(renderer.config.viewport_width, 1280);
        assert_eq!(renderer.config.viewport_height, 720);
        assert_eq!(renderer.config.timeout, Duration::from_secs(60));
        assert_eq!(renderer.config.user_agent, Some("TestBot/1.0".to_string()));
    }

    #[test]
    fn test_capabilities_firefox() {
        let renderer = WebDriverRendererBuilder::new()
            .firefox()
            .headless(true)
            .build();

        let caps = renderer.build_capabilities();
        assert_eq!(caps.get("browserName"), Some(&json!("firefox")));
    }

    #[test]
    fn test_capabilities_chrome() {
        let renderer = WebDriverRendererBuilder::new()
            .chrome()
            .headless(true)
            .build();

        let caps = renderer.build_capabilities();
        assert_eq!(caps.get("browserName"), Some(&json!("chrome")));
    }

    #[test]
    fn test_webdriver_cookie_builder() {
        let cookie = WebDriverCookie::new("session", "abc123")
            .domain(".example.com")
            .path("/")
            .secure(true)
            .http_only(true)
            .expiry(1234567890);

        assert_eq!(cookie.name, "session");
        assert_eq!(cookie.value, "abc123");
        assert_eq!(cookie.domain, Some(".example.com".to_string()));
        assert_eq!(cookie.path, Some("/".to_string()));
        assert_eq!(cookie.secure, Some(true));
        assert_eq!(cookie.http_only, Some(true));
        assert_eq!(cookie.expiry, Some(1234567890));
    }
}
