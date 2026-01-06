//! URL deduplication using Bloom filters
//!
//! Bloom filters provide memory-efficient probabilistic set membership testing.
//! A URL can be marked as "seen" and later checked with:
//! - False positives possible (URL not seen but reported as seen)
//! - No false negatives (URL seen is always reported as seen)

use std::sync::atomic::{AtomicU64, Ordering};

use bloomfilter::Bloom;
use parking_lot::RwLock;
use tracing::info;

/// Configuration for the URL deduplication filter
#[derive(Debug, Clone)]
pub struct DedupConfig {
    /// Expected number of URLs to store
    pub expected_items: usize,
    /// Target false positive rate (0.0 - 1.0)
    pub false_positive_rate: f64,
    /// Whether to use multiple bloom filters (for better accuracy with large datasets)
    pub use_partitioned: bool,
    /// Number of partitions if using partitioned mode
    pub partition_count: usize,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            expected_items: 10_000_000, // 10 million URLs
            false_positive_rate: 0.01,  // 1% false positive rate
            use_partitioned: false,
            partition_count: 16,
        }
    }
}

/// URL deduplication filter using Bloom filter
pub struct UrlDedup {
    config: DedupConfig,
    filter: RwLock<Bloom<str>>,
    count: AtomicU64,
}

impl UrlDedup {
    /// Create a new URL deduplication filter
    pub fn new(config: DedupConfig) -> Self {
        let filter = Bloom::new_for_fp_rate(config.expected_items, config.false_positive_rate);

        info!(
            expected_items = config.expected_items,
            fp_rate = config.false_positive_rate,
            bitmap_bits = filter.number_of_bits(),
            hash_functions = filter.number_of_hash_functions(),
            "Created URL dedup filter"
        );

        Self {
            config,
            filter: RwLock::new(filter),
            count: AtomicU64::new(0),
        }
    }

    /// Create a filter with default configuration
    pub fn with_defaults() -> Self {
        Self::new(DedupConfig::default())
    }

    /// Create a filter sized for a specific number of expected URLs
    pub fn for_capacity(expected_items: usize, fp_rate: f64) -> Self {
        Self::new(DedupConfig {
            expected_items,
            false_positive_rate: fp_rate,
            ..Default::default()
        })
    }

    /// Check if a URL has been seen (may have false positives)
    pub fn is_seen(&self, url: &str) -> bool {
        let filter = self.filter.read();
        filter.check(&normalize_url(url))
    }

    /// Mark a URL as seen
    pub fn mark_seen(&self, url: &str) {
        let normalized = normalize_url(url);
        let mut filter = self.filter.write();
        if !filter.check(&normalized) {
            filter.set(&normalized);
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Check and mark a URL as seen in one operation
    /// Returns true if the URL was already seen
    pub fn check_and_mark(&self, url: &str) -> bool {
        let normalized = normalize_url(url);
        let mut filter = self.filter.write();
        if filter.check(&normalized) {
            true
        } else {
            filter.set(&normalized);
            self.count.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// Mark multiple URLs as seen
    pub fn mark_seen_batch(&self, urls: &[String]) {
        let mut filter = self.filter.write();
        let mut new_count = 0u64;

        for url in urls {
            let normalized = normalize_url(url);
            if !filter.check(&normalized) {
                filter.set(&normalized);
                new_count += 1;
            }
        }

        self.count.fetch_add(new_count, Ordering::Relaxed);
    }

    /// Filter a list of URLs, returning only unseen ones
    pub fn filter_unseen(&self, urls: Vec<String>) -> Vec<String> {
        let filter = self.filter.read();
        urls.into_iter()
            .filter(|url| !filter.check(&normalize_url(url)))
            .collect()
    }

    /// Get the number of URLs marked as seen
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get estimated memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        let filter = self.filter.read();
        (filter.number_of_bits() / 8) as usize
    }

    /// Get statistics about the filter
    pub fn stats(&self) -> DedupStats {
        let filter = self.filter.read();
        DedupStats {
            items_count: self.count.load(Ordering::Relaxed),
            bitmap_bits: filter.number_of_bits(),
            hash_functions: filter.number_of_hash_functions(),
            estimated_memory_bytes: filter.number_of_bits() / 8,
            expected_capacity: self.config.expected_items,
            target_fp_rate: self.config.false_positive_rate,
        }
    }

    /// Clear the filter
    pub fn clear(&self) {
        let mut filter = self.filter.write();
        *filter =
            Bloom::new_for_fp_rate(self.config.expected_items, self.config.false_positive_rate);
        self.count.store(0, Ordering::Relaxed);
    }
}

/// Statistics about the deduplication filter
#[derive(Debug, Clone)]
pub struct DedupStats {
    /// Number of URLs marked as seen
    pub items_count: u64,
    /// Size of the bitmap in bits
    pub bitmap_bits: u64,
    /// Number of hash functions used
    pub hash_functions: u32,
    /// Estimated memory usage in bytes
    pub estimated_memory_bytes: u64,
    /// Expected capacity
    pub expected_capacity: usize,
    /// Target false positive rate
    pub target_fp_rate: f64,
}

/// Partitioned Bloom filter for very large datasets
/// Uses multiple bloom filters partitioned by URL hash for better performance
pub struct PartitionedUrlDedup {
    partitions: Vec<RwLock<Bloom<str>>>,
    partition_count: usize,
    count: AtomicU64,
}

impl PartitionedUrlDedup {
    /// Create a new partitioned dedup filter
    pub fn new(expected_items: usize, fp_rate: f64, partition_count: usize) -> Self {
        let items_per_partition = expected_items / partition_count + 1;

        let partitions: Vec<_> = (0..partition_count)
            .map(|_| RwLock::new(Bloom::new_for_fp_rate(items_per_partition, fp_rate)))
            .collect();

        info!(
            expected_items,
            fp_rate, partition_count, items_per_partition, "Created partitioned URL dedup filter"
        );

        Self {
            partitions,
            partition_count,
            count: AtomicU64::new(0),
        }
    }

    /// Get the partition index for a URL
    fn partition_index(&self, url: &str) -> usize {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        url.hash(&mut hasher);
        (hasher.finish() as usize) % self.partition_count
    }

    /// Check if a URL has been seen
    pub fn is_seen(&self, url: &str) -> bool {
        let normalized = normalize_url(url);
        let idx = self.partition_index(&normalized);
        let filter = self.partitions[idx].read();
        filter.check(&normalized)
    }

    /// Mark a URL as seen
    pub fn mark_seen(&self, url: &str) {
        let normalized = normalize_url(url);
        let idx = self.partition_index(&normalized);
        let mut filter = self.partitions[idx].write();
        if !filter.check(&normalized) {
            filter.set(&normalized);
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Check and mark a URL as seen
    pub fn check_and_mark(&self, url: &str) -> bool {
        let normalized = normalize_url(url);
        let idx = self.partition_index(&normalized);
        let mut filter = self.partitions[idx].write();
        if filter.check(&normalized) {
            true
        } else {
            filter.set(&normalized);
            self.count.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    /// Get the count of seen URLs
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }
}

/// Normalize a URL for deduplication
/// - Converts to lowercase
/// - Removes trailing slashes
/// - Removes fragments
/// - Normalizes common parameters
fn normalize_url(url: &str) -> String {
    let mut normalized = url.to_lowercase();

    // Remove fragment
    if let Some(idx) = normalized.find('#') {
        normalized.truncate(idx);
    }

    // Remove trailing slash (except for root)
    while normalized.ends_with('/')
        && normalized.len() > 1
        && normalized.chars().filter(|c| *c == '/').count() > 2
    {
        normalized.pop();
    }

    // Remove common tracking parameters
    if let Ok(mut parsed) = url::Url::parse(&normalized) {
        let tracking_params = [
            "utm_source",
            "utm_medium",
            "utm_campaign",
            "utm_content",
            "utm_term",
            "fbclid",
            "gclid",
        ];

        let query: Vec<_> = parsed
            .query_pairs()
            .filter(|(k, _)| !tracking_params.contains(&k.as_ref()))
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        if query.is_empty() {
            parsed.set_query(None);
        } else {
            parsed.set_query(Some(&query.join("&")));
        }

        normalized = parsed.to_string();
    }

    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_dedup() {
        let dedup = UrlDedup::for_capacity(1000, 0.01);

        assert!(!dedup.is_seen("https://example.com/page1"));

        dedup.mark_seen("https://example.com/page1");
        assert!(dedup.is_seen("https://example.com/page1"));

        assert!(!dedup.is_seen("https://example.com/page2"));
    }

    #[test]
    fn test_check_and_mark() {
        let dedup = UrlDedup::for_capacity(1000, 0.01);

        // First time - not seen
        assert!(!dedup.check_and_mark("https://example.com/page"));

        // Second time - already seen
        assert!(dedup.check_and_mark("https://example.com/page"));
    }

    #[test]
    fn test_url_normalization() {
        let dedup = UrlDedup::for_capacity(1000, 0.01);

        // Case insensitive
        dedup.mark_seen("https://EXAMPLE.COM/Page");
        assert!(dedup.is_seen("https://example.com/page"));

        // Trailing slashes
        dedup.mark_seen("https://example.com/other/");
        assert!(dedup.is_seen("https://example.com/other"));

        // Fragments removed
        dedup.mark_seen("https://example.com/doc#section");
        assert!(dedup.is_seen("https://example.com/doc"));
    }

    #[test]
    fn test_filter_unseen() {
        let dedup = UrlDedup::for_capacity(1000, 0.01);

        dedup.mark_seen("https://example.com/seen1");
        dedup.mark_seen("https://example.com/seen2");

        let urls = vec![
            "https://example.com/seen1".to_string(),
            "https://example.com/unseen1".to_string(),
            "https://example.com/seen2".to_string(),
            "https://example.com/unseen2".to_string(),
        ];

        let unseen = dedup.filter_unseen(urls);
        assert_eq!(unseen.len(), 2);
        assert!(unseen.contains(&"https://example.com/unseen1".to_string()));
        assert!(unseen.contains(&"https://example.com/unseen2".to_string()));
    }

    #[test]
    fn test_count() {
        let dedup = UrlDedup::for_capacity(1000, 0.01);

        dedup.mark_seen("https://example.com/1");
        dedup.mark_seen("https://example.com/2");
        dedup.mark_seen("https://example.com/1"); // Duplicate

        assert_eq!(dedup.count(), 2);
    }

    #[test]
    fn test_partitioned_dedup() {
        let dedup = PartitionedUrlDedup::new(1000, 0.01, 4);

        assert!(!dedup.is_seen("https://example.com/page"));
        dedup.mark_seen("https://example.com/page");
        assert!(dedup.is_seen("https://example.com/page"));
    }

    #[test]
    fn test_normalize_removes_tracking() {
        let url = "https://example.com/page?id=123&utm_source=google&utm_campaign=test";
        let normalized = normalize_url(url);
        assert!(normalized.contains("id=123"));
        assert!(!normalized.contains("utm_source"));
        assert!(!normalized.contains("utm_campaign"));
    }
}
