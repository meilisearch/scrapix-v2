//! Domain-based partitioning for distributed crawling
//!
//! Partitions URLs by domain to enable parallel crawling while
//! ensuring politeness (same domain processed by same worker).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use url::Url;

/// Configuration for partitioning
#[derive(Debug, Clone)]
pub struct PartitionConfig {
    /// Number of partitions
    pub partition_count: usize,
    /// Whether to use consistent hashing (for stable assignment)
    pub consistent_hashing: bool,
}

impl Default for PartitionConfig {
    fn default() -> Self {
        Self {
            partition_count: 16,
            consistent_hashing: true,
        }
    }
}

/// URL partitioner for distributing work across workers
pub struct Partitioner {
    config: PartitionConfig,
}

impl Partitioner {
    /// Create a new partitioner
    pub fn new(config: PartitionConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PartitionConfig::default())
    }

    /// Create with a specific number of partitions
    pub fn with_partitions(count: usize) -> Self {
        Self::new(PartitionConfig {
            partition_count: count,
            ..Default::default()
        })
    }

    /// Get the partition for a URL
    pub fn partition_for_url(&self, url: &str) -> usize {
        let domain = extract_domain(url);
        self.partition_for_domain(&domain)
    }

    /// Get the partition for a domain
    pub fn partition_for_domain(&self, domain: &str) -> usize {
        let normalized = normalize_domain(domain);
        let hash = hash_string(&normalized);
        (hash as usize) % self.config.partition_count
    }

    /// Get all URLs assigned to a specific partition
    pub fn filter_for_partition(&self, urls: Vec<String>, partition: usize) -> Vec<String> {
        urls.into_iter()
            .filter(|url| self.partition_for_url(url) == partition)
            .collect()
    }

    /// Group URLs by partition
    pub fn group_by_partition(&self, urls: Vec<String>) -> Vec<Vec<String>> {
        let mut groups: Vec<Vec<String>> = (0..self.config.partition_count)
            .map(|_| Vec::new())
            .collect();

        for url in urls {
            let partition = self.partition_for_url(&url);
            groups[partition].push(url);
        }

        groups
    }

    /// Get the number of partitions
    pub fn partition_count(&self) -> usize {
        self.config.partition_count
    }
}

/// Extract domain from a URL
pub fn extract_domain(url: &str) -> String {
    match Url::parse(url) {
        Ok(parsed) => parsed
            .host_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| url.to_string()),
        Err(_) => {
            // Try to extract domain from malformed URL
            let url = url
                .trim_start_matches("http://")
                .trim_start_matches("https://");
            url.split('/').next().unwrap_or(url).to_string()
        }
    }
}

/// Normalize a domain for consistent hashing
/// - Converts to lowercase
/// - Removes www. prefix
fn normalize_domain(domain: &str) -> String {
    let lower = domain.to_lowercase();
    lower.strip_prefix("www.").unwrap_or(&lower).to_string()
}

/// Hash a string using DefaultHasher
fn hash_string(s: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Domain grouper for batching requests to the same domain
pub struct DomainGrouper;

impl DomainGrouper {
    /// Group URLs by their domain
    pub fn group_by_domain(urls: Vec<String>) -> Vec<(String, Vec<String>)> {
        use std::collections::HashMap;

        let mut groups: HashMap<String, Vec<String>> = HashMap::new();

        for url in urls {
            let domain = extract_domain(&url);
            groups.entry(domain).or_default().push(url);
        }

        groups.into_iter().collect()
    }

    /// Group URLs by their registered domain (eTLD+1)
    /// e.g., sub.example.com and other.example.com -> example.com
    pub fn group_by_registered_domain(urls: Vec<String>) -> Vec<(String, Vec<String>)> {
        use std::collections::HashMap;

        let mut groups: HashMap<String, Vec<String>> = HashMap::new();

        for url in urls {
            let domain = extract_domain(&url);
            let registered = get_registered_domain(&domain);
            groups.entry(registered).or_default().push(url);
        }

        groups.into_iter().collect()
    }
}

/// Get the registered domain (simplified eTLD+1)
/// This is a simplified version - a full implementation would use the Public Suffix List
fn get_registered_domain(domain: &str) -> String {
    let parts: Vec<&str> = domain.split('.').collect();

    if parts.len() <= 2 {
        domain.to_string()
    } else {
        // Handle common cases
        let last_two = format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1]);

        // Check for known second-level domains
        let known_slds = ["co.uk", "co.jp", "com.au", "co.nz", "com.br"];
        if known_slds.contains(&last_two.as_str()) && parts.len() > 2 {
            format!("{}.{}", parts[parts.len() - 3], last_two)
        } else {
            last_two
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_for_url() {
        let partitioner = Partitioner::with_partitions(8);

        let partition1 = partitioner.partition_for_url("https://example.com/page1");
        let partition2 = partitioner.partition_for_url("https://example.com/page2");

        // Same domain should get same partition
        assert_eq!(partition1, partition2);

        // Different domains may get different partitions
        let _partition3 = partitioner.partition_for_url("https://other.com/page");
        // May or may not be different, depending on hash
    }

    #[test]
    fn test_www_normalization() {
        let partitioner = Partitioner::with_partitions(8);

        let p1 = partitioner.partition_for_url("https://example.com/page");
        let p2 = partitioner.partition_for_url("https://www.example.com/page");

        // www and non-www should get same partition
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://example.com/page"), "example.com");
        assert_eq!(
            extract_domain("http://sub.example.com/page"),
            "sub.example.com"
        );
        assert_eq!(
            extract_domain("https://example.com:8080/page"),
            "example.com"
        );
    }

    #[test]
    fn test_group_by_partition() {
        let partitioner = Partitioner::with_partitions(4);

        let urls = vec![
            "https://example.com/1".to_string(),
            "https://example.com/2".to_string(),
            "https://other.com/1".to_string(),
        ];

        let groups = partitioner.group_by_partition(urls);
        assert_eq!(groups.len(), 4);

        // example.com URLs should be in same group
        let example_partition = partitioner.partition_for_url("https://example.com/1");
        assert_eq!(groups[example_partition].len(), 2);
    }

    #[test]
    fn test_group_by_domain() {
        let urls = vec![
            "https://example.com/1".to_string(),
            "https://example.com/2".to_string(),
            "https://other.com/1".to_string(),
        ];

        let groups = DomainGrouper::group_by_domain(urls);
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn test_registered_domain() {
        assert_eq!(get_registered_domain("example.com"), "example.com");
        assert_eq!(get_registered_domain("sub.example.com"), "example.com");
        assert_eq!(get_registered_domain("deep.sub.example.com"), "example.com");
        assert_eq!(get_registered_domain("example.co.uk"), "example.co.uk");
        assert_eq!(get_registered_domain("sub.example.co.uk"), "example.co.uk");
    }
}
