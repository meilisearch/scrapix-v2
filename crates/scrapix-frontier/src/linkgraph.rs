//! Link graph for PageRank-like URL prioritization
//!
//! This module tracks link relationships between pages and computes importance
//! scores that can be used to boost URL priority in the crawl frontier.
//!
//! ## Example
//!
//! ```rust
//! use scrapix_frontier::{LinkGraph, LinkGraphConfig};
//!
//! // Create link graph
//! let graph = LinkGraph::new(LinkGraphConfig::default());
//!
//! // Record links discovered during crawl
//! graph.record_links(
//!     "https://example.com/",
//!     vec![
//!         "https://example.com/about",
//!         "https://example.com/contact",
//!     ],
//! );
//!
//! // Compute PageRank scores
//! graph.compute_scores();
//!
//! // Get priority boost for a URL
//! let boost = graph.get_priority_boost("https://example.com/about");
//! ```

use std::collections::{HashMap, HashSet};

use parking_lot::RwLock;

/// Configuration for the link graph
#[derive(Debug, Clone)]
pub struct LinkGraphConfig {
    /// Damping factor for PageRank (typically 0.85)
    pub damping_factor: f64,
    /// Number of iterations for PageRank computation
    pub iterations: u32,
    /// Minimum score threshold (pages below this get 0 boost)
    pub min_score_threshold: f64,
    /// Maximum priority boost to apply
    pub max_priority_boost: i32,
    /// Whether to normalize URLs before storing
    pub normalize_urls: bool,
    /// Maximum number of pages to track (0 = unlimited)
    pub max_pages: usize,
}

impl Default for LinkGraphConfig {
    fn default() -> Self {
        Self {
            damping_factor: 0.85,
            iterations: 20,
            min_score_threshold: 0.0001,
            max_priority_boost: 50,
            normalize_urls: true,
            max_pages: 0,
        }
    }
}

/// A node in the link graph representing a page
#[derive(Debug, Clone, Default)]
struct PageNode {
    /// URLs this page links to (outbound links)
    outbound: HashSet<String>,
    /// URLs that link to this page (inbound links)
    inbound: HashSet<String>,
    /// Computed PageRank score
    score: f64,
}

/// Link graph for tracking page relationships and computing importance scores
pub struct LinkGraph {
    config: LinkGraphConfig,
    /// Map from URL to page node
    pages: RwLock<HashMap<String, PageNode>>,
    /// Whether scores need recomputation
    dirty: RwLock<bool>,
}

impl LinkGraph {
    /// Create a new link graph
    pub fn new(config: LinkGraphConfig) -> Self {
        Self {
            config,
            pages: RwLock::new(HashMap::new()),
            dirty: RwLock::new(false),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(LinkGraphConfig::default())
    }

    /// Normalize a URL for consistent storage
    fn normalize_url(&self, url: &str) -> String {
        if !self.config.normalize_urls {
            return url.to_string();
        }

        // Basic normalization: lowercase scheme/host, remove trailing slash, remove fragment
        let url = url.trim();

        // Remove fragment
        let url = url.split('#').next().unwrap_or(url);

        // Remove trailing slash (except for root)
        let url = if url.ends_with('/') && url.matches('/').count() > 3 {
            &url[..url.len() - 1]
        } else {
            url
        };

        url.to_string()
    }

    /// Record links from a page
    ///
    /// Call this when a page is crawled to record its outbound links.
    pub fn record_links<S: AsRef<str>>(&self, source_url: &str, target_urls: Vec<S>) {
        let source = self.normalize_url(source_url);
        let targets: Vec<String> = target_urls
            .iter()
            .map(|u| self.normalize_url(u.as_ref()))
            .filter(|u| u != &source) // No self-links
            .collect();

        let mut pages = self.pages.write();

        // Check max pages limit
        if self.config.max_pages > 0 && pages.len() >= self.config.max_pages {
            // Only update existing pages, don't add new ones
            // First, filter targets to only existing ones
            let existing_targets: Vec<String> = targets
                .iter()
                .filter(|t| pages.contains_key(*t))
                .cloned()
                .collect();

            if let Some(source_node) = pages.get_mut(&source) {
                for target in &existing_targets {
                    source_node.outbound.insert(target.clone());
                }
            }
            // Update inbound links for existing target pages
            for target in &existing_targets {
                if let Some(target_node) = pages.get_mut(target) {
                    target_node.inbound.insert(source.clone());
                }
            }
        } else {
            // Get or create source node
            let source_node = pages.entry(source.clone()).or_default();
            for target in &targets {
                source_node.outbound.insert(target.clone());
            }

            // Update inbound links for targets
            for target in targets {
                let target_node = pages.entry(target).or_default();
                target_node.inbound.insert(source.clone());
            }
        }

        *self.dirty.write() = true;
    }

    /// Record a single link
    pub fn record_link(&self, source_url: &str, target_url: &str) {
        self.record_links(source_url, vec![target_url]);
    }

    /// Compute PageRank scores for all pages
    ///
    /// This should be called periodically (e.g., after each batch of pages)
    /// to update importance scores.
    pub fn compute_scores(&self) {
        let mut pages = self.pages.write();

        if pages.is_empty() {
            return;
        }

        let n = pages.len() as f64;
        let d = self.config.damping_factor;
        let base_score = (1.0 - d) / n;

        // Initialize all scores to 1/n
        for node in pages.values_mut() {
            node.score = 1.0 / n;
        }

        // Create a URL list for consistent ordering
        let urls: Vec<String> = pages.keys().cloned().collect();

        // Iterative PageRank computation
        for _ in 0..self.config.iterations {
            let mut new_scores: HashMap<String, f64> = HashMap::new();

            for url in &urls {
                let node = &pages[url];
                let mut score = base_score;

                // Sum contributions from inbound links
                for inbound_url in &node.inbound {
                    if let Some(inbound_node) = pages.get(inbound_url) {
                        let outbound_count = inbound_node.outbound.len() as f64;
                        if outbound_count > 0.0 {
                            score += d * (inbound_node.score / outbound_count);
                        }
                    }
                }

                new_scores.insert(url.clone(), score);
            }

            // Update scores
            for (url, score) in new_scores {
                if let Some(node) = pages.get_mut(&url) {
                    node.score = score;
                }
            }
        }

        *self.dirty.write() = false;
    }

    /// Compute scores if the graph has been modified
    pub fn compute_scores_if_dirty(&self) {
        if *self.dirty.read() {
            self.compute_scores();
        }
    }

    /// Get the PageRank score for a URL
    pub fn get_score(&self, url: &str) -> f64 {
        let url = self.normalize_url(url);
        let pages = self.pages.read();
        pages.get(&url).map(|n| n.score).unwrap_or(0.0)
    }

    /// Get priority boost for a URL based on its PageRank score
    ///
    /// Returns a value between 0 and `max_priority_boost` that can be
    /// added to the URL's priority.
    pub fn get_priority_boost(&self, url: &str) -> i32 {
        let score = self.get_score(url);

        if score < self.config.min_score_threshold {
            return 0;
        }

        // Scale score to priority boost
        // PageRank scores are typically small (1/n average), so we scale up
        let pages = self.pages.read();
        let n = pages.len() as f64;
        drop(pages);

        if n == 0.0 {
            return 0;
        }

        // Normalize: average score is 1/n, so score * n gives relative importance
        let normalized = score * n;

        // Apply log scaling to compress the range (many pages have low scores)
        let log_score = (1.0 + normalized).ln();

        // Scale to max boost
        let boost = (log_score * self.config.max_priority_boost as f64 / 5.0) as i32;

        boost.min(self.config.max_priority_boost).max(0)
    }

    /// Get the number of inbound links for a URL
    pub fn inbound_count(&self, url: &str) -> usize {
        let url = self.normalize_url(url);
        let pages = self.pages.read();
        pages.get(&url).map(|n| n.inbound.len()).unwrap_or(0)
    }

    /// Get the number of outbound links for a URL
    pub fn outbound_count(&self, url: &str) -> usize {
        let url = self.normalize_url(url);
        let pages = self.pages.read();
        pages.get(&url).map(|n| n.outbound.len()).unwrap_or(0)
    }

    /// Get the total number of pages in the graph
    pub fn page_count(&self) -> usize {
        self.pages.read().len()
    }

    /// Get the total number of links in the graph
    pub fn link_count(&self) -> usize {
        let pages = self.pages.read();
        pages.values().map(|n| n.outbound.len()).sum()
    }

    /// Get statistics about the link graph
    pub fn stats(&self) -> LinkGraphStats {
        self.compute_scores_if_dirty();

        let pages = self.pages.read();

        let page_count = pages.len();
        let link_count: usize = pages.values().map(|n| n.outbound.len()).sum();

        let (min_score, max_score, total_score) = if page_count > 0 {
            let scores: Vec<f64> = pages.values().map(|n| n.score).collect();
            let min = scores.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let total: f64 = scores.iter().sum();
            (min, max, total)
        } else {
            (0.0, 0.0, 0.0)
        };

        let avg_score = if page_count > 0 {
            total_score / page_count as f64
        } else {
            0.0
        };

        let (min_inbound, max_inbound, total_inbound) = if page_count > 0 {
            let inbounds: Vec<usize> = pages.values().map(|n| n.inbound.len()).collect();
            let min = *inbounds.iter().min().unwrap_or(&0);
            let max = *inbounds.iter().max().unwrap_or(&0);
            let total: usize = inbounds.iter().sum();
            (min, max, total)
        } else {
            (0, 0, 0)
        };

        let avg_inbound = if page_count > 0 {
            total_inbound as f64 / page_count as f64
        } else {
            0.0
        };

        LinkGraphStats {
            page_count,
            link_count,
            min_score,
            max_score,
            avg_score,
            min_inbound,
            max_inbound,
            avg_inbound,
        }
    }

    /// Get top N pages by PageRank score
    pub fn top_pages(&self, n: usize) -> Vec<(String, f64)> {
        self.compute_scores_if_dirty();

        let pages = self.pages.read();
        let mut scored: Vec<(String, f64)> = pages
            .iter()
            .map(|(url, node)| (url.clone(), node.score))
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(n);
        scored
    }

    /// Get pages with most inbound links
    pub fn most_linked(&self, n: usize) -> Vec<(String, usize)> {
        let pages = self.pages.read();
        let mut linked: Vec<(String, usize)> = pages
            .iter()
            .map(|(url, node)| (url.clone(), node.inbound.len()))
            .collect();

        linked.sort_by(|a, b| b.1.cmp(&a.1));
        linked.truncate(n);
        linked
    }

    /// Clear the link graph
    pub fn clear(&self) {
        self.pages.write().clear();
        *self.dirty.write() = false;
    }

    /// Check if a URL exists in the graph
    pub fn contains(&self, url: &str) -> bool {
        let url = self.normalize_url(url);
        self.pages.read().contains_key(&url)
    }

    /// Get all URLs that link to a given URL
    pub fn get_inbound_links(&self, url: &str) -> Vec<String> {
        let url = self.normalize_url(url);
        let pages = self.pages.read();
        pages
            .get(&url)
            .map(|n| n.inbound.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Get all URLs that a given URL links to
    pub fn get_outbound_links(&self, url: &str) -> Vec<String> {
        let url = self.normalize_url(url);
        let pages = self.pages.read();
        pages
            .get(&url)
            .map(|n| n.outbound.iter().cloned().collect())
            .unwrap_or_default()
    }
}

/// Statistics about the link graph
#[derive(Debug, Clone)]
pub struct LinkGraphStats {
    /// Number of pages in the graph
    pub page_count: usize,
    /// Total number of links
    pub link_count: usize,
    /// Minimum PageRank score
    pub min_score: f64,
    /// Maximum PageRank score
    pub max_score: f64,
    /// Average PageRank score
    pub avg_score: f64,
    /// Minimum inbound links
    pub min_inbound: usize,
    /// Maximum inbound links
    pub max_inbound: usize,
    /// Average inbound links
    pub avg_inbound: f64,
}

/// Builder for LinkGraph
pub struct LinkGraphBuilder {
    config: LinkGraphConfig,
}

impl LinkGraphBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: LinkGraphConfig::default(),
        }
    }

    /// Set damping factor
    pub fn damping_factor(mut self, factor: f64) -> Self {
        self.config.damping_factor = factor.clamp(0.0, 1.0);
        self
    }

    /// Set number of iterations
    pub fn iterations(mut self, iterations: u32) -> Self {
        self.config.iterations = iterations;
        self
    }

    /// Set minimum score threshold
    pub fn min_score_threshold(mut self, threshold: f64) -> Self {
        self.config.min_score_threshold = threshold;
        self
    }

    /// Set maximum priority boost
    pub fn max_priority_boost(mut self, boost: i32) -> Self {
        self.config.max_priority_boost = boost;
        self
    }

    /// Set URL normalization
    pub fn normalize_urls(mut self, normalize: bool) -> Self {
        self.config.normalize_urls = normalize;
        self
    }

    /// Set maximum pages to track
    pub fn max_pages(mut self, max: usize) -> Self {
        self.config.max_pages = max;
        self
    }

    /// Build the link graph
    pub fn build(self) -> LinkGraph {
        LinkGraph::new(self.config)
    }
}

impl Default for LinkGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_links() {
        let graph = LinkGraph::with_defaults();

        graph.record_links(
            "https://example.com/",
            vec!["https://example.com/about", "https://example.com/contact"],
        );

        assert_eq!(graph.page_count(), 3);
        assert_eq!(graph.outbound_count("https://example.com/"), 2);
        assert_eq!(graph.inbound_count("https://example.com/about"), 1);
    }

    #[test]
    fn test_pagerank_simple() {
        let graph = LinkGraph::with_defaults();

        // Simple graph: A -> B -> C, A -> C
        graph.record_link("https://a.com", "https://b.com");
        graph.record_link("https://b.com", "https://c.com");
        graph.record_link("https://a.com", "https://c.com");

        graph.compute_scores();

        // C should have highest score (most inbound links)
        let score_a = graph.get_score("https://a.com");
        let score_b = graph.get_score("https://b.com");
        let score_c = graph.get_score("https://c.com");

        assert!(score_c > score_b, "C should rank higher than B");
        assert!(score_c > score_a, "C should rank higher than A");
    }

    #[test]
    fn test_pagerank_hub() {
        let graph = LinkGraph::with_defaults();

        // Hub pattern: many pages link to one page
        let hub = "https://example.com/hub";
        for i in 0..10 {
            graph.record_link(&format!("https://example.com/page{}", i), hub);
        }

        graph.compute_scores();

        let hub_score = graph.get_score(hub);
        let page_score = graph.get_score("https://example.com/page0");

        assert!(
            hub_score > page_score,
            "Hub should have higher score: {} vs {}",
            hub_score,
            page_score
        );
    }

    #[test]
    fn test_priority_boost() {
        let graph = LinkGraph::with_defaults();

        // Create a hub with many inbound links
        let hub = "https://example.com/hub";
        for i in 0..20 {
            graph.record_link(&format!("https://example.com/page{}", i), hub);
        }

        graph.compute_scores();

        let hub_boost = graph.get_priority_boost(hub);
        let page_boost = graph.get_priority_boost("https://example.com/page0");

        assert!(hub_boost > page_boost, "Hub should get higher boost");
        assert!(hub_boost <= graph.config.max_priority_boost);
    }

    #[test]
    fn test_url_normalization() {
        let graph = LinkGraph::with_defaults();

        graph.record_link("https://example.com/page/", "https://example.com/other");
        graph.record_link("https://example.com/page", "https://example.com/another");

        // Both should be normalized to same URL
        assert_eq!(graph.outbound_count("https://example.com/page"), 2);
    }

    #[test]
    fn test_no_self_links() {
        let graph = LinkGraph::with_defaults();

        graph.record_links(
            "https://example.com/page",
            vec![
                "https://example.com/page",  // Self-link
                "https://example.com/other", // Valid link
            ],
        );

        // Self-link should be filtered out
        assert_eq!(graph.outbound_count("https://example.com/page"), 1);
    }

    #[test]
    fn test_stats() {
        let graph = LinkGraph::with_defaults();

        graph.record_link("https://a.com", "https://b.com");
        graph.record_link("https://a.com", "https://c.com");
        graph.record_link("https://b.com", "https://c.com");

        let stats = graph.stats();

        assert_eq!(stats.page_count, 3);
        assert_eq!(stats.link_count, 3);
        assert!(stats.avg_score > 0.0);
    }

    #[test]
    fn test_top_pages() {
        let graph = LinkGraph::with_defaults();

        // C gets most links
        graph.record_link("https://a.com", "https://c.com");
        graph.record_link("https://b.com", "https://c.com");
        graph.record_link("https://a.com", "https://b.com");

        graph.compute_scores();

        let top = graph.top_pages(2);
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].0, "https://c.com");
    }

    #[test]
    fn test_most_linked() {
        let graph = LinkGraph::with_defaults();

        let hub = "https://hub.com";
        for i in 0..5 {
            graph.record_link(&format!("https://page{}.com", i), hub);
        }

        let most = graph.most_linked(1);
        assert_eq!(most.len(), 1);
        assert_eq!(most[0].0, hub);
        assert_eq!(most[0].1, 5);
    }

    #[test]
    fn test_builder() {
        let graph = LinkGraphBuilder::new()
            .damping_factor(0.9)
            .iterations(30)
            .max_priority_boost(100)
            .build();

        assert_eq!(graph.config.damping_factor, 0.9);
        assert_eq!(graph.config.iterations, 30);
        assert_eq!(graph.config.max_priority_boost, 100);
    }

    #[test]
    fn test_fragment_removal() {
        let graph = LinkGraph::with_defaults();

        graph.record_link(
            "https://example.com/page#section1",
            "https://example.com/other#section2",
        );

        // Fragments should be stripped
        assert!(graph.contains("https://example.com/page"));
        assert!(graph.contains("https://example.com/other"));
    }

    #[test]
    fn test_inbound_outbound_links() {
        let graph = LinkGraph::with_defaults();

        graph.record_link("https://a.com", "https://b.com");
        graph.record_link("https://a.com", "https://c.com");
        graph.record_link("https://b.com", "https://c.com");

        let outbound = graph.get_outbound_links("https://a.com");
        assert_eq!(outbound.len(), 2);

        let inbound = graph.get_inbound_links("https://c.com");
        assert_eq!(inbound.len(), 2);
    }

    #[test]
    fn test_empty_graph() {
        let graph = LinkGraph::with_defaults();

        assert_eq!(graph.page_count(), 0);
        assert_eq!(graph.get_score("https://any.com"), 0.0);
        assert_eq!(graph.get_priority_boost("https://any.com"), 0);

        let stats = graph.stats();
        assert_eq!(stats.page_count, 0);
    }
}
