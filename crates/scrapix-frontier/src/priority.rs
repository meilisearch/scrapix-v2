//! Priority queue for URL scheduling
//!
//! URLs are prioritized based on:
//! - Explicit priority value
//! - Crawl depth (lower depth = higher priority)
//! - Domain importance

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use parking_lot::Mutex;

use scrapix_core::CrawlUrl;

/// A prioritized URL entry
#[derive(Debug, Clone)]
struct PrioritizedUrl {
    url: CrawlUrl,
    score: i64,
}

impl PartialEq for PrioritizedUrl {
    fn eq(&self, other: &Self) -> bool {
        self.score == other.score
    }
}

impl Eq for PrioritizedUrl {}

impl PartialOrd for PrioritizedUrl {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedUrl {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher score = higher priority
        self.score.cmp(&other.score)
    }
}

/// Configuration for the priority queue
#[derive(Debug, Clone)]
pub struct PriorityConfig {
    /// Maximum queue size (0 = unlimited)
    pub max_size: usize,
    /// Weight for explicit priority (higher = more important)
    pub priority_weight: i64,
    /// Weight for depth (negative because lower depth = higher priority)
    pub depth_weight: i64,
    /// Base score for seed URLs (depth 0)
    pub seed_bonus: i64,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        Self {
            max_size: 0,
            priority_weight: 100,
            depth_weight: -10,
            seed_bonus: 1000,
        }
    }
}

/// Priority queue for URLs
pub struct PriorityQueue {
    config: PriorityConfig,
    heap: Mutex<BinaryHeap<PrioritizedUrl>>,
}

impl PriorityQueue {
    /// Create a new priority queue
    pub fn new(config: PriorityConfig) -> Self {
        Self {
            config,
            heap: Mutex::new(BinaryHeap::new()),
        }
    }

    /// Create a queue with default configuration
    pub fn with_defaults() -> Self {
        Self::new(PriorityConfig::default())
    }

    /// Calculate priority score for a URL
    fn calculate_score(&self, url: &CrawlUrl) -> i64 {
        let mut score = 0i64;

        // Add explicit priority
        score += (url.priority as i64) * self.config.priority_weight;

        // Penalize depth
        score += (url.depth as i64) * self.config.depth_weight;

        // Bonus for seed URLs
        if url.depth == 0 {
            score += self.config.seed_bonus;
        }

        score
    }

    /// Push a URL onto the queue
    pub fn push(&self, url: CrawlUrl) {
        let mut heap = self.heap.lock();

        // Check max size
        if self.config.max_size > 0 && heap.len() >= self.config.max_size {
            // Check if new URL has higher priority than lowest
            let score = self.calculate_score(&url);
            if let Some(lowest) = heap.peek() {
                if score <= lowest.score {
                    return; // Reject lower priority URL
                }
            }
            // Make room by removing lowest priority
            // Note: BinaryHeap doesn't directly support this, so we skip for now
        }

        let score = self.calculate_score(&url);
        heap.push(PrioritizedUrl { url, score });
    }

    /// Push multiple URLs
    pub fn push_many(&self, urls: Vec<CrawlUrl>) {
        let mut heap = self.heap.lock();

        for url in urls {
            let score = self.calculate_score(&url);
            heap.push(PrioritizedUrl { url, score });
        }
    }

    /// Pop the highest priority URL
    pub fn pop(&self) -> Option<CrawlUrl> {
        let mut heap = self.heap.lock();
        heap.pop().map(|p| p.url)
    }

    /// Pop up to N URLs
    pub fn pop_many(&self, count: usize) -> Vec<CrawlUrl> {
        let mut heap = self.heap.lock();
        let mut urls = Vec::with_capacity(count.min(heap.len()));

        for _ in 0..count {
            match heap.pop() {
                Some(p) => urls.push(p.url),
                None => break,
            }
        }

        urls
    }

    /// Peek at the highest priority URL without removing it
    pub fn peek(&self) -> Option<CrawlUrl> {
        let heap = self.heap.lock();
        heap.peek().map(|p| p.url.clone())
    }

    /// Get the queue length
    pub fn len(&self) -> usize {
        self.heap.lock().len()
    }

    /// Check if the queue is empty
    pub fn is_empty(&self) -> bool {
        self.heap.lock().is_empty()
    }

    /// Clear the queue
    pub fn clear(&self) {
        self.heap.lock().clear();
    }
}

/// Multi-level priority queue with separate queues for different priority ranges
pub struct MultiLevelPriorityQueue {
    /// High priority queue (priority >= 10)
    high: PriorityQueue,
    /// Normal priority queue (0 <= priority < 10)
    normal: PriorityQueue,
    /// Low priority queue (priority < 0)
    low: PriorityQueue,
    /// Weights for selecting from each queue [high, normal, low]
    weights: [u32; 3],
    /// Counter for round-robin selection
    counter: Mutex<u32>,
}

impl MultiLevelPriorityQueue {
    /// Create a new multi-level priority queue
    pub fn new(weights: [u32; 3]) -> Self {
        Self {
            high: PriorityQueue::with_defaults(),
            normal: PriorityQueue::with_defaults(),
            low: PriorityQueue::with_defaults(),
            weights,
            counter: Mutex::new(0),
        }
    }

    /// Create with default weights [5, 3, 1] (high, normal, low)
    pub fn with_defaults() -> Self {
        Self::new([5, 3, 1])
    }

    /// Push a URL to the appropriate queue
    pub fn push(&self, url: CrawlUrl) {
        if url.priority >= 10 {
            self.high.push(url);
        } else if url.priority >= 0 {
            self.normal.push(url);
        } else {
            self.low.push(url);
        }
    }

    /// Pop from the queues using weighted selection
    pub fn pop(&self) -> Option<CrawlUrl> {
        let mut counter = self.counter.lock();
        let total_weight = self.weights[0] + self.weights[1] + self.weights[2];
        let selection = *counter % total_weight;
        *counter = counter.wrapping_add(1);
        drop(counter);

        // Weighted selection
        if selection < self.weights[0] {
            self.high
                .pop()
                .or_else(|| self.normal.pop())
                .or_else(|| self.low.pop())
        } else if selection < self.weights[0] + self.weights[1] {
            self.normal
                .pop()
                .or_else(|| self.high.pop())
                .or_else(|| self.low.pop())
        } else {
            self.low
                .pop()
                .or_else(|| self.normal.pop())
                .or_else(|| self.high.pop())
        }
    }

    /// Get total length across all queues
    pub fn len(&self) -> usize {
        self.high.len() + self.normal.len() + self.low.len()
    }

    /// Check if all queues are empty
    pub fn is_empty(&self) -> bool {
        self.high.is_empty() && self.normal.is_empty() && self.low.is_empty()
    }

    /// Get lengths of individual queues
    pub fn queue_lengths(&self) -> (usize, usize, usize) {
        (self.high.len(), self.normal.len(), self.low.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_url(url: &str, depth: u32, priority: i32) -> CrawlUrl {
        CrawlUrl {
            url: url.to_string(),
            depth,
            priority,
            parent_url: None,
            anchor_text: None,
            discovered_at: Utc::now(),
            retry_count: 0,
            requires_js: false,
        }
    }

    #[test]
    fn test_priority_ordering() {
        let queue = PriorityQueue::with_defaults();

        queue.push(make_url("https://example.com/low", 2, 0));
        queue.push(make_url("https://example.com/high", 0, 10));
        queue.push(make_url("https://example.com/medium", 1, 5));

        // Should get highest priority (seed with priority 10) first
        let first = queue.pop().unwrap();
        assert!(first.url.contains("high"));

        let second = queue.pop().unwrap();
        assert!(second.url.contains("medium") || second.url.contains("low"));
    }

    #[test]
    fn test_depth_affects_priority() {
        let queue = PriorityQueue::with_defaults();

        // Same explicit priority, different depths
        queue.push(make_url("https://example.com/deep", 5, 0));
        queue.push(make_url("https://example.com/shallow", 1, 0));

        // Shallow should come first (lower depth = higher priority)
        let first = queue.pop().unwrap();
        assert!(first.url.contains("shallow"));
    }

    #[test]
    fn test_push_many() {
        let queue = PriorityQueue::with_defaults();

        let urls = vec![
            make_url("https://example.com/1", 0, 0),
            make_url("https://example.com/2", 0, 0),
            make_url("https://example.com/3", 0, 0),
        ];

        queue.push_many(urls);
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn test_pop_many() {
        let queue = PriorityQueue::with_defaults();

        for i in 0..5 {
            queue.push(make_url(&format!("https://example.com/{}", i), 0, 0));
        }

        let popped = queue.pop_many(3);
        assert_eq!(popped.len(), 3);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_multi_level_queue() {
        let queue = MultiLevelPriorityQueue::with_defaults();

        queue.push(make_url("https://example.com/high", 0, 10));
        queue.push(make_url("https://example.com/normal", 0, 5));
        queue.push(make_url("https://example.com/low", 0, -5));

        assert_eq!(queue.len(), 3);

        let (h, n, l) = queue.queue_lengths();
        assert_eq!(h, 1);
        assert_eq!(n, 1);
        assert_eq!(l, 1);
    }
}
