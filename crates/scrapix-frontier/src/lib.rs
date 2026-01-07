//! # Scrapix Frontier
//!
//! URL frontier service with deduplication and politeness scheduling.
//!
//! ## Features
//!
//! - Bloom filter-based URL deduplication (memory-efficient)
//! - Domain-based partitioning for distributed crawling
//! - Priority queue with depth tracking
//! - Politeness scheduling (per-domain rate limiting)
//! - Adaptive backoff on errors
//!
//! ## Memory Efficiency
//!
//! Using Bloom filters instead of hash sets provides ~90% memory savings:
//! - 10M URLs with hash set: ~1GB
//! - 10M URLs with Bloom filter: ~15MB (at 1% false positive rate)
//!
//! ## Example
//!
//! ```rust,no_run
//! use scrapix_frontier::{
//!     UrlDedup, DedupConfig,
//!     PriorityQueue, PriorityConfig,
//!     PolitenessScheduler, PolitenessConfig,
//!     Partitioner,
//! };
//! use scrapix_core::CrawlUrl;
//!
//! // Create deduplication filter for 10M URLs
//! let dedup = UrlDedup::for_capacity(10_000_000, 0.01);
//!
//! // Create priority queue
//! let queue = PriorityQueue::with_defaults();
//!
//! // Create politeness scheduler
//! let scheduler = PolitenessScheduler::with_defaults();
//!
//! // Create partitioner for 8 workers
//! let partitioner = Partitioner::with_partitions(8);
//!
//! // Add a URL if not seen
//! let url = "https://example.com/page";
//! if !dedup.check_and_mark(url) {
//!     queue.push(CrawlUrl::seed(url));
//! }
//!
//! // Get next URL to crawl
//! if let Some(url) = queue.pop() {
//!     let domain = "example.com";
//!     if scheduler.can_fetch(domain) {
//!         scheduler.start_request(domain);
//!         // ... fetch URL ...
//!         scheduler.complete_request(domain);
//!     }
//! }
//! ```

pub mod dedup;
pub mod history;
pub mod linkgraph;
pub mod partition;
pub mod politeness;
pub mod priority;
pub mod recrawl;
pub mod simhash;

// Re-exports
pub use dedup::{DedupConfig, DedupStats, PartitionedUrlDedup, UrlDedup};
pub use history::{
    check_content_change, fingerprint_bytes, fingerprint_content, ConditionalHeaders,
    ContentChangeResult, CrawlRecord, UrlHistory, UrlHistoryConfig, UrlHistoryStats,
};
pub use partition::{extract_domain, DomainGrouper, PartitionConfig, Partitioner};
pub use politeness::{DomainStats, PolitenessConfig, PolitenessScheduler};
pub use priority::{MultiLevelPriorityQueue, PriorityConfig, PriorityQueue};
pub use linkgraph::{LinkGraph, LinkGraphBuilder, LinkGraphConfig, LinkGraphStats};
pub use recrawl::{
    RecrawlConfig, RecrawlDecision, RecrawlReason, RecrawlScheduler, RecrawlSchedulerBuilder,
    RecrawlStats, SkipReason,
};
pub use simhash::{
    DuplicateCluster, DuplicateClusterer, MinHash, NearDuplicateConfig, NearDuplicateDetector,
    NearDuplicateStats, SimHash,
};
