//! URL extraction from HTML pages

use std::collections::HashSet;

use scraper::{Html, Selector};
use tracing::debug;
use url::Url;

use scrapix_core::{CrawlUrl, RawPage, UrlPatterns};

/// URL extraction configuration
#[derive(Debug, Clone)]
pub struct ExtractorConfig {
    /// URL patterns for filtering
    pub patterns: Option<UrlPatterns>,
    /// Maximum depth to follow links
    pub max_depth: u32,
    /// Whether to follow external links (different domain)
    pub follow_external: bool,
    /// Whether to follow subdomains
    pub follow_subdomains: bool,
    /// Whether to extract links from specific attributes
    pub extract_from_data_attrs: bool,
}

impl Default for ExtractorConfig {
    fn default() -> Self {
        Self {
            patterns: None,
            max_depth: u32::MAX,
            follow_external: false,
            follow_subdomains: true,
            extract_from_data_attrs: false,
        }
    }
}

/// URL extractor for finding links in HTML
pub struct UrlExtractor {
    config: ExtractorConfig,
    link_selector: Selector,
}

impl UrlExtractor {
    /// Create a new URL extractor
    pub fn new(config: ExtractorConfig) -> Self {
        // Selector for anchor tags with href
        let link_selector = Selector::parse("a[href]").unwrap();

        Self {
            config,
            link_selector,
        }
    }

    /// Create a new URL extractor with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ExtractorConfig::default())
    }

    /// Extract URLs from a page
    pub fn extract(&self, page: &RawPage, parent_depth: u32) -> Vec<CrawlUrl> {
        if parent_depth >= self.config.max_depth {
            return vec![];
        }

        let base_url = match Url::parse(&page.final_url) {
            Ok(url) => url,
            Err(_) => return vec![],
        };

        let base_domain = base_url.host_str().unwrap_or("").to_lowercase();

        let document = Html::parse_document(&page.html);
        let mut seen = HashSet::new();
        let mut urls = Vec::new();

        for element in document.select(&self.link_selector) {
            if let Some(href) = element.value().attr("href") {
                if let Some(crawl_url) = self.process_href(
                    href,
                    &base_url,
                    &base_domain,
                    &page.final_url,
                    parent_depth,
                    &mut seen,
                ) {
                    // Get anchor text
                    let anchor_text = element
                        .text()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string();

                    let crawl_url = if anchor_text.is_empty() {
                        crawl_url
                    } else {
                        CrawlUrl {
                            anchor_text: Some(anchor_text),
                            ..crawl_url
                        }
                    };

                    urls.push(crawl_url);
                }
            }
        }

        debug!(
            url = %page.final_url,
            extracted = urls.len(),
            "Extracted URLs"
        );

        urls
    }

    /// Extract URLs as strings only (for implementing Parser trait)
    pub fn extract_urls(&self, page: &RawPage) -> Vec<String> {
        let base_url = match Url::parse(&page.final_url) {
            Ok(url) => url,
            Err(_) => return vec![],
        };

        let base_domain = base_url.host_str().unwrap_or("").to_lowercase();
        let document = Html::parse_document(&page.html);
        let mut seen = HashSet::new();
        let mut urls = Vec::new();

        for element in document.select(&self.link_selector) {
            if let Some(href) = element.value().attr("href") {
                if let Some(url) = self.resolve_and_filter(href, &base_url, &base_domain, &mut seen)
                {
                    urls.push(url);
                }
            }
        }

        urls
    }

    /// Process a single href attribute
    fn process_href(
        &self,
        href: &str,
        base_url: &Url,
        base_domain: &str,
        parent_url: &str,
        parent_depth: u32,
        seen: &mut HashSet<String>,
    ) -> Option<CrawlUrl> {
        let resolved = self.resolve_and_filter(href, base_url, base_domain, seen)?;

        Some(CrawlUrl {
            url: resolved,
            depth: parent_depth + 1,
            priority: 0,
            parent_url: Some(parent_url.to_string()),
            anchor_text: None,
            discovered_at: chrono::Utc::now(),
            retry_count: 0,
            requires_js: false,
        })
    }

    /// Resolve a URL and apply filters
    fn resolve_and_filter(
        &self,
        href: &str,
        base_url: &Url,
        base_domain: &str,
        seen: &mut HashSet<String>,
    ) -> Option<String> {
        // Skip empty, javascript:, mailto:, tel:, etc.
        let href = href.trim();
        if href.is_empty()
            || href.starts_with("javascript:")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
            || href.starts_with("data:")
            || href.starts_with('#')
        {
            return None;
        }

        // Resolve relative URLs
        let resolved = match base_url.join(href) {
            Ok(url) => url,
            Err(_) => return None,
        };

        // Normalize the URL
        let normalized = self.normalize_url(&resolved)?;

        // Check if already seen
        if seen.contains(&normalized) {
            return None;
        }

        // Check domain constraints
        let url_domain = resolved.host_str()?.to_lowercase();
        if !self.is_allowed_domain(&url_domain, base_domain) {
            return None;
        }

        // Apply URL pattern filters
        if let Some(ref patterns) = self.config.patterns {
            if !self.matches_patterns(&normalized, patterns) {
                return None;
            }
        }

        seen.insert(normalized.clone());
        Some(normalized)
    }

    /// Normalize a URL (remove fragments, trailing slashes, etc.)
    fn normalize_url(&self, url: &Url) -> Option<String> {
        // Only allow HTTP/HTTPS
        if url.scheme() != "http" && url.scheme() != "https" {
            return None;
        }

        let mut normalized = url.clone();

        // Remove fragment
        normalized.set_fragment(None);

        // Convert to string
        let mut url_str = normalized.to_string();

        // Remove trailing slash for consistency (except for root)
        if url_str.ends_with('/') && url_str.chars().filter(|c| *c == '/').count() > 3 {
            url_str.pop();
        }

        Some(url_str)
    }

    /// Check if a domain is allowed based on configuration
    fn is_allowed_domain(&self, url_domain: &str, base_domain: &str) -> bool {
        if url_domain == base_domain {
            return true;
        }

        if self.config.follow_subdomains {
            // Check if url_domain is a subdomain of base_domain
            if url_domain.ends_with(&format!(".{}", base_domain)) {
                return true;
            }
            // Or if base_domain is a subdomain of url_domain
            if base_domain.ends_with(&format!(".{}", url_domain)) {
                return true;
            }
        }

        self.config.follow_external
    }

    /// Check if URL matches configured patterns
    fn matches_patterns(&self, url: &str, patterns: &UrlPatterns) -> bool {
        // Check exclude patterns first
        for pattern in &patterns.exclude {
            if self.matches_glob(url, pattern) {
                return false;
            }
        }

        // Check include patterns
        if patterns.include.is_empty() {
            return true;
        }
        for pattern in &patterns.include {
            if self.matches_glob(url, pattern) {
                return true;
            }
        }
        false
    }

    /// Simple glob-style pattern matching
    fn matches_glob(&self, url: &str, pattern: &str) -> bool {
        // Convert glob to simple matching
        // * matches any characters except /
        // ** matches any characters including /

        if pattern.contains("**") {
            // Handle ** as "match anything"
            let parts: Vec<&str> = pattern.split("**").collect();
            if parts.len() == 2 {
                return url.starts_with(parts[0])
                    && (parts[1].is_empty() || url.ends_with(parts[1]));
            }
        }

        if pattern.contains('*') {
            // Handle * as "match anything except /"
            let parts: Vec<&str> = pattern.split('*').collect();
            let mut pos = 0;
            for part in parts {
                if part.is_empty() {
                    continue;
                }
                if let Some(found) = url[pos..].find(part) {
                    // Check no / between pos and found
                    if url[pos..pos + found].contains('/') && !pattern.contains("**") {
                        return false;
                    }
                    pos = pos + found + part.len();
                } else {
                    return false;
                }
            }
            true
        } else {
            // Exact match
            url == pattern
        }
    }
}

/// Builder for UrlExtractor
pub struct UrlExtractorBuilder {
    config: ExtractorConfig,
}

impl UrlExtractorBuilder {
    pub fn new() -> Self {
        Self {
            config: ExtractorConfig::default(),
        }
    }

    pub fn patterns(mut self, patterns: UrlPatterns) -> Self {
        self.config.patterns = Some(patterns);
        self
    }

    pub fn max_depth(mut self, depth: u32) -> Self {
        self.config.max_depth = depth;
        self
    }

    pub fn follow_external(mut self, follow: bool) -> Self {
        self.config.follow_external = follow;
        self
    }

    pub fn follow_subdomains(mut self, follow: bool) -> Self {
        self.config.follow_subdomains = follow;
        self
    }

    pub fn build(self) -> UrlExtractor {
        UrlExtractor::new(self.config)
    }
}

impl Default for UrlExtractorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_page(url: &str, html: &str) -> RawPage {
        RawPage {
            url: url.to_string(),
            final_url: url.to_string(),
            status: 200,
            headers: Default::default(),
            html: html.to_string(),
            content_type: Some("text/html".to_string()),
            js_rendered: false,
            fetched_at: chrono::Utc::now(),
            fetch_duration_ms: 100,
        }
    }

    #[test]
    fn test_extract_absolute_urls() {
        let extractor = UrlExtractor::with_defaults();
        let page = make_page(
            "https://example.com",
            "<html><body><a href=\"https://example.com/page1\">Page 1</a><a href=\"https://example.com/page2\">Page 2</a></body></html>",
        );

        let urls = extractor.extract(&page, 0);
        assert_eq!(urls.len(), 2);
        assert!(urls.iter().any(|u| u.url == "https://example.com/page1"));
        assert!(urls.iter().any(|u| u.url == "https://example.com/page2"));
    }

    #[test]
    fn test_extract_relative_urls() {
        let extractor = UrlExtractor::with_defaults();
        let page = make_page(
            "https://example.com/section/",
            "<html><body><a href=\"/page1\">Page 1</a><a href=\"page2\">Page 2</a><a href=\"../other\">Other</a></body></html>",
        );

        let urls = extractor.extract_urls(&page);
        assert!(urls.contains(&"https://example.com/page1".to_string()));
        assert!(urls.contains(&"https://example.com/section/page2".to_string()));
        assert!(urls.contains(&"https://example.com/other".to_string()));
    }

    #[test]
    fn test_skip_javascript_urls() {
        let extractor = UrlExtractor::with_defaults();
        let page = make_page(
            "https://example.com",
            "<html><body><a href=\"javascript:void(0)\">Click</a><a href=\"mailto:test@example.com\">Email</a><a href=\"#section\">Anchor</a><a href=\"https://example.com/valid\">Valid</a></body></html>",
        );

        let urls = extractor.extract_urls(&page);
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://example.com/valid");
    }

    #[test]
    fn test_external_links_blocked_by_default() {
        let extractor = UrlExtractor::with_defaults();
        let page = make_page(
            "https://example.com",
            "<html><body><a href=\"https://example.com/internal\">Internal</a><a href=\"https://other.com/external\">External</a></body></html>",
        );

        let urls = extractor.extract_urls(&page);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("example.com"));
    }

    #[test]
    fn test_subdomain_following() {
        let extractor = UrlExtractor::new(ExtractorConfig {
            follow_subdomains: true,
            ..Default::default()
        });
        let page = make_page(
            "https://example.com",
            "<html><body><a href=\"https://blog.example.com/post\">Blog</a><a href=\"https://other.com/page\">Other</a></body></html>",
        );

        let urls = extractor.extract_urls(&page);
        assert_eq!(urls.len(), 1);
        assert!(urls[0].contains("blog.example.com"));
    }

    #[test]
    fn test_glob_patterns() {
        let extractor = UrlExtractor::new(ExtractorConfig {
            patterns: Some(UrlPatterns {
                include: vec!["https://example.com/docs/**".to_string()],
                exclude: vec!["**/_internal/**".to_string()],
                index_only: vec![],
            }),
            ..Default::default()
        });

        assert!(extractor.matches_glob(
            "https://example.com/docs/page",
            "https://example.com/docs/**"
        ));
        assert!(extractor.matches_glob(
            "https://example.com/docs/deep/page",
            "https://example.com/docs/**"
        ));
        assert!(!extractor.matches_glob(
            "https://example.com/blog/page",
            "https://example.com/docs/**"
        ));
    }
}
