//! Sitemap parsing and discovery
//!
//! Supports:
//! - Standard XML sitemaps (urlset)
//! - Sitemap index files (sitemapindex)
//! - Auto-discovery from robots.txt
//! - Priority and lastmod extraction
//!
//! ## Example
//!
//! ```rust,no_run
//! use scrapix_crawler::sitemap::{SitemapParser, SitemapConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let parser = SitemapParser::new(SitemapConfig::default());
//!
//!     // Parse a sitemap
//!     let urls = parser.fetch_and_parse("https://example.com/sitemap.xml").await?;
//!     for url in urls {
//!         println!("{} (priority: {:?})", url.loc, url.priority);
//!     }
//!
//!     // Auto-discover from robots.txt
//!     let sitemaps = parser.discover_from_robots("https://example.com").await?;
//!     Ok(())
//! }
//! ```

use std::io::BufRead;
use std::time::Duration;

use chrono::{DateTime, Utc};
use quick_xml::events::Event;
use quick_xml::Reader;
use reqwest::Client;
use tracing::{debug, instrument, warn};
use url::Url;

use scrapix_core::{Result, ScrapixError};

/// Configuration for sitemap parsing
#[derive(Debug, Clone)]
pub struct SitemapConfig {
    /// User agent for requests
    pub user_agent: String,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum sitemap size in bytes (default 50MB)
    pub max_size: usize,
    /// Maximum URLs to extract from a single sitemap
    pub max_urls: usize,
    /// Maximum depth for sitemap index recursion
    pub max_depth: u32,
    /// Follow sitemap index files
    pub follow_index: bool,
}

impl Default for SitemapConfig {
    fn default() -> Self {
        Self {
            user_agent: "Scrapix/1.0".to_string(),
            timeout: Duration::from_secs(30),
            max_size: 50 * 1024 * 1024, // 50MB
            max_urls: 100_000,
            max_depth: 3,
            follow_index: true,
        }
    }
}

/// A URL entry from a sitemap
#[derive(Debug, Clone)]
pub struct SitemapUrl {
    /// The URL location
    pub loc: String,
    /// Last modification time
    pub lastmod: Option<DateTime<Utc>>,
    /// Change frequency hint
    pub changefreq: Option<ChangeFrequency>,
    /// Priority (0.0 to 1.0)
    pub priority: Option<f32>,
}

impl SitemapUrl {
    /// Create a new sitemap URL
    pub fn new(loc: impl Into<String>) -> Self {
        Self {
            loc: loc.into(),
            lastmod: None,
            changefreq: None,
            priority: None,
        }
    }

    /// Get the priority or default (0.5)
    pub fn priority_or_default(&self) -> f32 {
        self.priority.unwrap_or(0.5)
    }
}

/// Change frequency hint from sitemap
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeFrequency {
    Always,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Never,
}

impl ChangeFrequency {
    /// Parse from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "always" => Some(Self::Always),
            "hourly" => Some(Self::Hourly),
            "daily" => Some(Self::Daily),
            "weekly" => Some(Self::Weekly),
            "monthly" => Some(Self::Monthly),
            "yearly" => Some(Self::Yearly),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    /// Convert to approximate seconds
    pub fn to_seconds(&self) -> Option<u64> {
        match self {
            Self::Always => Some(0),
            Self::Hourly => Some(3600),
            Self::Daily => Some(86400),
            Self::Weekly => Some(604800),
            Self::Monthly => Some(2592000),
            Self::Yearly => Some(31536000),
            Self::Never => None,
        }
    }
}

/// A sitemap entry (URL to a sitemap file)
#[derive(Debug, Clone)]
pub struct SitemapEntry {
    /// Location of the sitemap
    pub loc: String,
    /// Last modification time
    pub lastmod: Option<DateTime<Utc>>,
}

/// Result of parsing a sitemap
#[derive(Debug)]
pub enum SitemapContent {
    /// A regular sitemap with URLs
    UrlSet(Vec<SitemapUrl>),
    /// A sitemap index with links to other sitemaps
    Index(Vec<SitemapEntry>),
}

/// Sitemap parser
pub struct SitemapParser {
    config: SitemapConfig,
    client: Client,
}

impl SitemapParser {
    /// Create a new sitemap parser
    pub fn new(config: SitemapConfig) -> Self {
        let client = Client::builder()
            .timeout(config.timeout)
            .user_agent(&config.user_agent)
            .gzip(true)
            .build()
            .expect("Failed to build HTTP client");

        Self { config, client }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SitemapConfig::default())
    }

    /// Discover sitemaps from robots.txt
    #[instrument(skip(self))]
    pub async fn discover_from_robots(&self, base_url: &str) -> Result<Vec<String>> {
        let parsed = Url::parse(base_url)?;
        let robots_url = format!(
            "{}://{}/robots.txt",
            parsed.scheme(),
            parsed.host_str().unwrap_or("")
        );

        debug!(robots_url, "Fetching robots.txt for sitemap discovery");

        let response = self
            .client
            .get(&robots_url)
            .send()
            .await
            .map_err(|e| ScrapixError::Crawl(format!("Failed to fetch robots.txt: {}", e)))?;

        if !response.status().is_success() {
            // No robots.txt, try default sitemap location
            let default_sitemap = format!(
                "{}://{}/sitemap.xml",
                parsed.scheme(),
                parsed.host_str().unwrap_or("")
            );
            return Ok(vec![default_sitemap]);
        }

        let content = response
            .text()
            .await
            .map_err(|e| ScrapixError::Crawl(format!("Failed to read robots.txt: {}", e)))?;

        let mut sitemaps = Vec::new();

        // Parse Sitemap: directives from robots.txt
        for line in content.lines() {
            let line = line.trim();
            if line.to_lowercase().starts_with("sitemap:") {
                let sitemap_url = line[8..].trim();
                if !sitemap_url.is_empty() {
                    // Resolve relative URLs
                    if let Ok(resolved) = parsed.join(sitemap_url) {
                        sitemaps.push(resolved.to_string());
                    } else {
                        sitemaps.push(sitemap_url.to_string());
                    }
                }
            }
        }

        // If no sitemaps found in robots.txt, try default location
        if sitemaps.is_empty() {
            let default_sitemap = format!(
                "{}://{}/sitemap.xml",
                parsed.scheme(),
                parsed.host_str().unwrap_or("")
            );
            sitemaps.push(default_sitemap);
        }

        debug!(count = sitemaps.len(), "Discovered sitemaps");
        Ok(sitemaps)
    }

    /// Fetch and parse a sitemap, following index files if configured
    #[instrument(skip(self))]
    pub async fn fetch_and_parse(&self, url: &str) -> Result<Vec<SitemapUrl>> {
        self.fetch_and_parse_recursive(url, 0).await
    }

    /// Recursive sitemap parsing with depth limit
    fn fetch_and_parse_recursive<'a>(
        &'a self,
        url: &'a str,
        depth: u32,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<SitemapUrl>>> + Send + 'a>>
    {
        Box::pin(async move {
            if depth > self.config.max_depth {
                warn!(url, depth, "Max sitemap depth exceeded");
                return Ok(Vec::new());
            }

            debug!(url, depth, "Fetching sitemap");

            let content = self.fetch_sitemap(url).await?;
            let parsed = self.parse_xml(&content)?;

            match parsed {
                SitemapContent::UrlSet(urls) => {
                    let count = urls.len().min(self.config.max_urls);
                    debug!(url, count, "Parsed URL sitemap");
                    Ok(urls.into_iter().take(self.config.max_urls).collect())
                }
                SitemapContent::Index(entries) => {
                    if !self.config.follow_index {
                        debug!(url, "Sitemap index found but follow_index is disabled");
                        return Ok(Vec::new());
                    }

                    debug!(url, count = entries.len(), "Parsed sitemap index");

                    let mut all_urls = Vec::new();
                    for entry in entries {
                        match self.fetch_and_parse_recursive(&entry.loc, depth + 1).await {
                            Ok(urls) => {
                                all_urls.extend(urls);
                                if all_urls.len() >= self.config.max_urls {
                                    break;
                                }
                            }
                            Err(e) => {
                                warn!(sitemap = entry.loc, error = %e, "Failed to parse sub-sitemap");
                            }
                        }
                    }

                    Ok(all_urls.into_iter().take(self.config.max_urls).collect())
                }
            }
        })
    }

    /// Fetch sitemap content
    async fn fetch_sitemap(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ScrapixError::Crawl(format!("Failed to fetch sitemap: {}", e)))?;

        if !response.status().is_success() {
            return Err(ScrapixError::Crawl(format!(
                "Sitemap returned status {}",
                response.status()
            )));
        }

        // Check content length
        if let Some(len) = response.content_length() {
            if len as usize > self.config.max_size {
                return Err(ScrapixError::Crawl(format!(
                    "Sitemap too large: {} bytes (max {})",
                    len, self.config.max_size
                )));
            }
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ScrapixError::Crawl(format!("Failed to read sitemap: {}", e)))?;

        // Check if gzipped (magic bytes: 1f 8b)
        if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
            // Decompress gzip
            let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
            let mut content = String::new();
            std::io::Read::read_to_string(&mut decoder, &mut content)
                .map_err(|e| ScrapixError::Crawl(format!("Failed to decompress sitemap: {}", e)))?;
            Ok(content)
        } else {
            String::from_utf8(bytes.to_vec())
                .map_err(|e| ScrapixError::Crawl(format!("Invalid UTF-8 in sitemap: {}", e)))
        }
    }

    /// Parse sitemap XML content
    fn parse_xml(&self, content: &str) -> Result<SitemapContent> {
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        // Detect sitemap type by looking for root element
        let mut buf = Vec::new();
        let mut is_index = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    let local = name.local_name();
                    if local.as_ref() == b"sitemapindex" {
                        is_index = true;
                    }
                    break;
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(ScrapixError::Crawl(format!("XML parse error: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        // Reset reader
        let mut reader = Reader::from_str(content);
        reader.config_mut().trim_text(true);

        if is_index {
            self.parse_sitemap_index(&mut reader)
        } else {
            self.parse_urlset(&mut reader)
        }
    }

    /// Parse a urlset sitemap
    fn parse_urlset<B: BufRead>(&self, reader: &mut Reader<B>) -> Result<SitemapContent> {
        let mut urls = Vec::new();
        let mut buf = Vec::new();

        let mut current_url: Option<SitemapUrl> = None;
        let mut current_tag = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    let local = String::from_utf8_lossy(name.local_name().as_ref()).to_string();
                    current_tag = local.clone();

                    if local == "url" {
                        current_url = Some(SitemapUrl::new(""));
                    }
                }
                Ok(Event::Text(e)) => {
                    if let Some(ref mut url) = current_url {
                        let text = e.unescape().unwrap_or_default().to_string();
                        match current_tag.as_str() {
                            "loc" => url.loc = text,
                            "lastmod" => {
                                url.lastmod = parse_datetime(&text);
                            }
                            "changefreq" => {
                                url.changefreq = ChangeFrequency::parse(&text);
                            }
                            "priority" => {
                                url.priority = text.parse().ok();
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    let name = e.name();
                    let local = String::from_utf8_lossy(name.local_name().as_ref()).to_string();
                    if local == "url" {
                        if let Some(url) = current_url.take() {
                            if !url.loc.is_empty() {
                                urls.push(url);
                            }
                        }
                    }
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!(error = %e, "XML parse error, returning partial results");
                    break;
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(SitemapContent::UrlSet(urls))
    }

    /// Parse a sitemap index
    fn parse_sitemap_index<B: BufRead>(&self, reader: &mut Reader<B>) -> Result<SitemapContent> {
        let mut entries = Vec::new();
        let mut buf = Vec::new();

        let mut current_entry: Option<SitemapEntry> = None;
        let mut current_tag = String::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => {
                    let name = e.name();
                    let local = String::from_utf8_lossy(name.local_name().as_ref()).to_string();
                    current_tag = local.clone();

                    if local == "sitemap" {
                        current_entry = Some(SitemapEntry {
                            loc: String::new(),
                            lastmod: None,
                        });
                    }
                }
                Ok(Event::Text(e)) => {
                    if let Some(ref mut entry) = current_entry {
                        let text = e.unescape().unwrap_or_default().to_string();
                        match current_tag.as_str() {
                            "loc" => entry.loc = text,
                            "lastmod" => {
                                entry.lastmod = parse_datetime(&text);
                            }
                            _ => {}
                        }
                    }
                }
                Ok(Event::End(e)) => {
                    let name = e.name();
                    let local = String::from_utf8_lossy(name.local_name().as_ref()).to_string();
                    if local == "sitemap" {
                        if let Some(entry) = current_entry.take() {
                            if !entry.loc.is_empty() {
                                entries.push(entry);
                            }
                        }
                    }
                    current_tag.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    warn!(error = %e, "XML parse error, returning partial results");
                    break;
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(SitemapContent::Index(entries))
    }

    /// Discover and fetch all URLs from a domain's sitemaps
    #[instrument(skip(self))]
    pub async fn discover_all_urls(&self, base_url: &str) -> Result<Vec<SitemapUrl>> {
        let sitemaps = self.discover_from_robots(base_url).await?;

        let mut all_urls = Vec::new();
        for sitemap in sitemaps {
            match self.fetch_and_parse(&sitemap).await {
                Ok(urls) => {
                    all_urls.extend(urls);
                    if all_urls.len() >= self.config.max_urls {
                        break;
                    }
                }
                Err(e) => {
                    warn!(sitemap, error = %e, "Failed to parse sitemap");
                }
            }
        }

        // Sort by priority (highest first)
        all_urls.sort_by(|a, b| {
            b.priority_or_default()
                .partial_cmp(&a.priority_or_default())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(all_urls.into_iter().take(self.config.max_urls).collect())
    }
}

/// Parse various datetime formats used in sitemaps
fn parse_datetime(s: &str) -> Option<DateTime<Utc>> {
    // Try ISO 8601 formats
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try YYYY-MM-DD format
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(date.and_hms_opt(0, 0, 0)?.and_utc());
    }

    // Try YYYY-MM-DDTHH:MM:SS format without timezone
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
        return Some(dt.and_utc());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_urlset() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
            <url>
                <loc>https://example.com/page1</loc>
                <lastmod>2024-01-15</lastmod>
                <changefreq>weekly</changefreq>
                <priority>0.8</priority>
            </url>
            <url>
                <loc>https://example.com/page2</loc>
                <priority>0.5</priority>
            </url>
        </urlset>"#;

        let parser = SitemapParser::with_defaults();
        let result = parser.parse_xml(xml).unwrap();

        match result {
            SitemapContent::UrlSet(urls) => {
                assert_eq!(urls.len(), 2);
                assert_eq!(urls[0].loc, "https://example.com/page1");
                assert_eq!(urls[0].priority, Some(0.8));
                assert!(urls[0].lastmod.is_some());
                assert_eq!(urls[0].changefreq, Some(ChangeFrequency::Weekly));
                assert_eq!(urls[1].loc, "https://example.com/page2");
            }
            _ => panic!("Expected UrlSet"),
        }
    }

    #[test]
    fn test_parse_sitemap_index() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
            <sitemap>
                <loc>https://example.com/sitemap1.xml</loc>
                <lastmod>2024-01-15</lastmod>
            </sitemap>
            <sitemap>
                <loc>https://example.com/sitemap2.xml</loc>
            </sitemap>
        </sitemapindex>"#;

        let parser = SitemapParser::with_defaults();
        let result = parser.parse_xml(xml).unwrap();

        match result {
            SitemapContent::Index(entries) => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].loc, "https://example.com/sitemap1.xml");
                assert!(entries[0].lastmod.is_some());
            }
            _ => panic!("Expected Index"),
        }
    }

    #[test]
    fn test_change_frequency() {
        assert_eq!(
            ChangeFrequency::parse("daily"),
            Some(ChangeFrequency::Daily)
        );
        assert_eq!(
            ChangeFrequency::parse("WEEKLY"),
            Some(ChangeFrequency::Weekly)
        );
        assert_eq!(ChangeFrequency::parse("invalid"), None);

        assert_eq!(ChangeFrequency::Daily.to_seconds(), Some(86400));
        assert_eq!(ChangeFrequency::Never.to_seconds(), None);
    }

    #[test]
    fn test_parse_datetime() {
        // ISO 8601 with timezone
        assert!(parse_datetime("2024-01-15T10:30:00+00:00").is_some());
        // Date only
        assert!(parse_datetime("2024-01-15").is_some());
        // ISO without timezone
        assert!(parse_datetime("2024-01-15T10:30:00").is_some());
        // Invalid
        assert!(parse_datetime("not-a-date").is_none());
    }
}
