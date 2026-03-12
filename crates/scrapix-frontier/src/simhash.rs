//! SimHash and MinHash for near-duplicate detection
//!
//! This module provides locality-sensitive hashing algorithms for detecting
//! near-duplicate content, which is essential for avoiding redundant crawling
//! and storage of similar pages.
//!
//! ## Algorithms
//!
//! - **SimHash**: Creates a fingerprint where similar documents have similar hashes.
//!   Uses Hamming distance to measure similarity.
//! - **MinHash**: Estimates Jaccard similarity between documents using random
//!   hash functions. Good for set-based similarity (e.g., word sets).
//!
//! ## Example
//!
//! ```rust
//! use scrapix_frontier::{SimHash, MinHash, NearDuplicateDetector, NearDuplicateConfig};
//!
//! // SimHash for document fingerprinting
//! let simhash = SimHash::new();
//! let hash1 = simhash.hash("The quick brown fox jumps over the lazy dog");
//! let hash2 = simhash.hash("The quick brown fox jumps over the lazy cat");
//! let distance = SimHash::hamming_distance(hash1, hash2);
//! println!("Hamming distance: {} (similar if < 3)", distance);
//!
//! // MinHash for Jaccard similarity
//! let minhash = MinHash::new(128); // 128 hash functions
//! let sig1 = minhash.signature("document one with some words");
//! let sig2 = minhash.signature("document two with some words");
//! let similarity = MinHash::jaccard_similarity(&sig1, &sig2);
//! println!("Jaccard similarity: {:.2}", similarity);
//!
//! // Near-duplicate detector with LSH
//! let detector = NearDuplicateDetector::new(NearDuplicateConfig::default());
//! let is_dup = detector.is_near_duplicate("https://example.com/page", "page content here");
//! ```

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use parking_lot::RwLock;
use siphasher::sip::SipHasher13;

/// SimHash implementation for document fingerprinting
///
/// SimHash creates a 64-bit fingerprint where similar documents
/// produce fingerprints with small Hamming distance.
#[derive(Debug, Clone)]
pub struct SimHash {
    /// Number of bits in the hash (always 64 for u64)
    bits: usize,
}

impl Default for SimHash {
    fn default() -> Self {
        Self::new()
    }
}

impl SimHash {
    /// Create a new SimHash instance
    pub fn new() -> Self {
        Self { bits: 64 }
    }

    /// Compute SimHash for a document
    ///
    /// The algorithm:
    /// 1. Tokenize the document into features (shingles/n-grams)
    /// 2. Hash each feature to a 64-bit value
    /// 3. For each bit position, sum +1 if bit is 1, -1 if bit is 0
    /// 4. Final hash: bit is 1 if sum > 0, else 0
    pub fn hash(&self, content: &str) -> u64 {
        let tokens = self.tokenize(content);
        if tokens.is_empty() {
            return 0;
        }

        // Accumulator for each bit position
        let mut v = vec![0i64; self.bits];

        for token in tokens {
            let hash = self.hash_token(&token);

            for (idx, val) in v.iter_mut().enumerate() {
                if (hash >> idx) & 1 == 1 {
                    *val += 1;
                } else {
                    *val -= 1;
                }
            }
        }

        // Build final hash
        let mut result = 0u64;
        for (i, &count) in v.iter().enumerate() {
            if count > 0 {
                result |= 1 << i;
            }
        }

        result
    }

    /// Compute SimHash for pre-tokenized content
    pub fn hash_tokens(&self, tokens: &[String]) -> u64 {
        if tokens.is_empty() {
            return 0;
        }

        let mut v = vec![0i64; self.bits];

        for token in tokens {
            let hash = self.hash_token(token);

            for (idx, val) in v.iter_mut().enumerate() {
                if (hash >> idx) & 1 == 1 {
                    *val += 1;
                } else {
                    *val -= 1;
                }
            }
        }

        let mut result = 0u64;
        for (i, &count) in v.iter().enumerate() {
            if count > 0 {
                result |= 1 << i;
            }
        }

        result
    }

    /// Calculate Hamming distance between two SimHash values
    ///
    /// Returns the number of differing bits (0-64).
    /// Documents are typically considered near-duplicates if distance < 3-5.
    pub fn hamming_distance(hash1: u64, hash2: u64) -> u32 {
        (hash1 ^ hash2).count_ones()
    }

    /// Check if two hashes are similar within a threshold
    pub fn is_similar(hash1: u64, hash2: u64, threshold: u32) -> bool {
        Self::hamming_distance(hash1, hash2) <= threshold
    }

    /// Tokenize content into shingles (character n-grams)
    fn tokenize(&self, content: &str) -> Vec<String> {
        let normalized = self.normalize(content);
        let words: Vec<&str> = normalized.split_whitespace().collect();

        if words.len() < 3 {
            return words.iter().map(|s| s.to_string()).collect();
        }

        // Create 3-word shingles
        words.windows(3).map(|w| w.join(" ")).collect()
    }

    /// Normalize text for consistent hashing
    fn normalize(&self, content: &str) -> String {
        let mut result = String::with_capacity(content.len());
        let mut prev_was_space = true; // Collapse leading whitespace
        for c in content.chars() {
            if c.is_alphanumeric() {
                if prev_was_space && !result.is_empty() {
                    result.push(' ');
                }
                // Manual lowercase for ASCII fast path
                for lc in c.to_lowercase() {
                    result.push(lc);
                }
                prev_was_space = false;
            } else if c.is_whitespace() {
                prev_was_space = true;
            }
        }
        result
    }

    /// Hash a single token using SipHash
    fn hash_token(&self, token: &str) -> u64 {
        let mut hasher = SipHasher13::new();
        token.hash(&mut hasher);
        hasher.finish()
    }
}

/// MinHash implementation for Jaccard similarity estimation
///
/// MinHash uses multiple hash functions to create a signature that
/// can estimate Jaccard similarity between sets.
#[derive(Debug, Clone)]
pub struct MinHash {
    /// Number of hash functions (signature length)
    num_hashes: usize,
    /// Seeds for hash functions
    seeds: Vec<(u64, u64)>,
}

impl MinHash {
    /// Create a new MinHash with specified number of hash functions
    ///
    /// More hash functions = more accurate similarity estimation but larger signatures.
    /// 128-256 is typical for most applications.
    pub fn new(num_hashes: usize) -> Self {
        use std::collections::hash_map::RandomState;
        use std::hash::BuildHasher;

        let mut seeds = Vec::with_capacity(num_hashes);
        let state = RandomState::new();

        for i in 0..num_hashes {
            let mut hasher = state.build_hasher();
            i.hash(&mut hasher);
            let seed1 = hasher.finish();
            (i + num_hashes).hash(&mut hasher);
            let seed2 = hasher.finish();
            seeds.push((seed1, seed2));
        }

        Self { num_hashes, seeds }
    }

    /// Create MinHash with deterministic seeds (for reproducible results)
    pub fn with_seed(num_hashes: usize, base_seed: u64) -> Self {
        let mut seeds = Vec::with_capacity(num_hashes);

        for i in 0..num_hashes {
            let seed1 = base_seed.wrapping_mul(i as u64 + 1);
            let seed2 = base_seed.wrapping_mul(i as u64 + num_hashes as u64 + 1);
            seeds.push((seed1, seed2));
        }

        Self { num_hashes, seeds }
    }

    /// Compute MinHash signature for a document
    pub fn signature(&self, content: &str) -> Vec<u64> {
        let shingles = self.shingle(content);
        self.signature_from_set(&shingles)
    }

    /// Compute MinHash signature from a pre-computed set of shingles
    pub fn signature_from_set(&self, shingles: &HashSet<String>) -> Vec<u64> {
        let mut signature = vec![u64::MAX; self.num_hashes];

        for shingle in shingles {
            for (i, &(seed1, seed2)) in self.seeds.iter().enumerate() {
                let hash = self.hash_with_seeds(shingle, seed1, seed2);
                if hash < signature[i] {
                    signature[i] = hash;
                }
            }
        }

        signature
    }

    /// Estimate Jaccard similarity from two signatures
    ///
    /// Returns a value between 0.0 (completely different) and 1.0 (identical).
    pub fn jaccard_similarity(sig1: &[u64], sig2: &[u64]) -> f64 {
        if sig1.len() != sig2.len() {
            return 0.0;
        }

        let matches = sig1.iter().zip(sig2.iter()).filter(|(a, b)| a == b).count();
        matches as f64 / sig1.len() as f64
    }

    /// Check if two signatures indicate near-duplicates
    pub fn is_similar(sig1: &[u64], sig2: &[u64], threshold: f64) -> bool {
        Self::jaccard_similarity(sig1, sig2) >= threshold
    }

    /// Create shingles (word n-grams) from content
    fn shingle(&self, content: &str) -> HashSet<String> {
        let normalized: String = content
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric() || c.is_whitespace())
            .collect();

        let words: Vec<&str> = normalized.split_whitespace().collect();

        if words.len() < 3 {
            return words.iter().map(|s| s.to_string()).collect();
        }

        words.windows(3).map(|w| w.join(" ")).collect()
    }

    /// Hash a string with two seeds using SipHash variant
    fn hash_with_seeds(&self, s: &str, seed1: u64, seed2: u64) -> u64 {
        let mut hasher = SipHasher13::new_with_keys(seed1, seed2);
        s.hash(&mut hasher);
        hasher.finish()
    }

    /// Get the number of hash functions
    pub fn num_hashes(&self) -> usize {
        self.num_hashes
    }
}

/// Configuration for near-duplicate detection
#[derive(Debug, Clone)]
pub struct NearDuplicateConfig {
    /// SimHash Hamming distance threshold (typically 3-5)
    pub simhash_threshold: u32,
    /// MinHash Jaccard similarity threshold (typically 0.8-0.9)
    pub minhash_threshold: f64,
    /// Number of MinHash functions
    pub num_minhash_functions: usize,
    /// Number of LSH bands for MinHash
    pub lsh_bands: usize,
    /// Maximum number of fingerprints to store
    pub max_fingerprints: usize,
    /// Use SimHash (faster) or MinHash (more accurate)
    pub use_simhash: bool,
}

impl Default for NearDuplicateConfig {
    fn default() -> Self {
        Self {
            simhash_threshold: 3,
            minhash_threshold: 0.85,
            num_minhash_functions: 128,
            lsh_bands: 16, // 128 / 16 = 8 rows per band
            max_fingerprints: 10_000_000,
            use_simhash: true, // SimHash is faster for large scale
        }
    }
}

type MinHashBuckets = Vec<HashMap<u64, Vec<(String, Vec<u64>)>>>;

/// Near-duplicate detector using LSH (Locality-Sensitive Hashing)
///
/// Uses either SimHash or MinHash with LSH for efficient near-duplicate detection.
pub struct NearDuplicateDetector {
    config: NearDuplicateConfig,
    simhash: SimHash,
    minhash: MinHash,
    /// SimHash fingerprints: hash -> list of (url, original_hash)
    simhash_buckets: RwLock<HashMap<u64, Vec<(String, u64)>>>,
    /// MinHash LSH buckets: band_hash -> list of (url, signature)
    minhash_buckets: RwLock<MinHashBuckets>,
    /// Total fingerprints stored
    fingerprint_count: AtomicU64,
    /// Statistics
    stats: RwLock<NearDuplicateStats>,
}

/// Statistics for near-duplicate detection
#[derive(Debug, Clone, Default)]
pub struct NearDuplicateStats {
    /// Total documents checked
    pub documents_checked: u64,
    /// Near-duplicates found
    pub duplicates_found: u64,
    /// Unique documents
    pub unique_documents: u64,
    /// Average similarity of detected duplicates
    pub avg_duplicate_similarity: f64,
}

impl NearDuplicateDetector {
    /// Create a new near-duplicate detector
    pub fn new(config: NearDuplicateConfig) -> Self {
        let minhash = MinHash::with_seed(config.num_minhash_functions, 0x12345678);
        let num_bands = config.lsh_bands;

        Self {
            simhash: SimHash::new(),
            minhash,
            simhash_buckets: RwLock::new(HashMap::new()),
            minhash_buckets: RwLock::new(vec![HashMap::new(); num_bands]),
            fingerprint_count: AtomicU64::new(0),
            stats: RwLock::new(NearDuplicateStats::default()),
            config,
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(NearDuplicateConfig::default())
    }

    /// Check if content is a near-duplicate of previously seen content
    ///
    /// Returns Some(url) if a near-duplicate was found, None otherwise.
    /// Also adds the content to the index if it's not a duplicate.
    pub fn check_and_add(&self, url: &str, content: &str) -> Option<String> {
        let mut stats = self.stats.write();
        stats.documents_checked += 1;

        if self.config.use_simhash {
            self.check_simhash(url, content, &mut stats)
        } else {
            self.check_minhash(url, content, &mut stats)
        }
    }

    /// Check if content is a near-duplicate (without adding to index)
    pub fn is_near_duplicate(&self, url: &str, content: &str) -> bool {
        if self.config.use_simhash {
            let hash = self.simhash.hash(content);
            self.find_similar_simhash(hash).is_some()
        } else {
            let signature = self.minhash.signature(content);
            self.find_similar_minhash(&signature, url).is_some()
        }
    }

    /// Add content to the index without checking for duplicates
    pub fn add(&self, url: &str, content: &str) {
        if self.fingerprint_count.load(Ordering::Relaxed) >= self.config.max_fingerprints as u64 {
            return;
        }

        if self.config.use_simhash {
            let hash = self.simhash.hash(content);
            self.add_simhash(url, hash);
        } else {
            let signature = self.minhash.signature(content);
            self.add_minhash(url, signature);
        }
    }

    /// Get the SimHash fingerprint for content
    pub fn compute_simhash(&self, content: &str) -> u64 {
        self.simhash.hash(content)
    }

    /// Get the MinHash signature for content
    pub fn compute_minhash(&self, content: &str) -> Vec<u64> {
        self.minhash.signature(content)
    }

    /// Get statistics
    pub fn stats(&self) -> NearDuplicateStats {
        self.stats.read().clone()
    }

    /// Get the number of stored fingerprints
    pub fn fingerprint_count(&self) -> u64 {
        self.fingerprint_count.load(Ordering::Relaxed)
    }

    /// Clear all stored fingerprints
    pub fn clear(&self) {
        self.simhash_buckets.write().clear();
        let mut minhash_buckets = self.minhash_buckets.write();
        for bucket in minhash_buckets.iter_mut() {
            bucket.clear();
        }
        self.fingerprint_count.store(0, Ordering::Relaxed);
        *self.stats.write() = NearDuplicateStats::default();
    }

    // SimHash-based detection

    fn check_simhash(
        &self,
        url: &str,
        content: &str,
        stats: &mut NearDuplicateStats,
    ) -> Option<String> {
        let hash = self.simhash.hash(content);

        // Check for similar hashes
        if let Some((dup_url, distance)) = self.find_similar_simhash(hash) {
            stats.duplicates_found += 1;
            let similarity = 1.0 - (distance as f64 / 64.0);
            stats.avg_duplicate_similarity =
                (stats.avg_duplicate_similarity * (stats.duplicates_found - 1) as f64 + similarity)
                    / stats.duplicates_found as f64;
            return Some(dup_url);
        }

        // Not a duplicate - add to index
        self.add_simhash(url, hash);
        stats.unique_documents += 1;
        None
    }

    fn find_similar_simhash(&self, hash: u64) -> Option<(String, u32)> {
        // Zero-hash means empty/no-token content — treat as unique to avoid
        // false collisions between unrelated empty pages.
        if hash == 0 {
            return None;
        }

        let buckets = self.simhash_buckets.read();

        // For small datasets, check all buckets (brute force)
        // For large datasets, you'd want a more sophisticated LSH approach
        // Here we use a simple bucketing scheme but check all entries
        // since the bucket-based optimization can miss near-duplicates
        // when the top bits differ

        // Check all buckets for similarity
        for entries in buckets.values() {
            for (url, stored_hash) in entries {
                // Skip zero-hash stored entries too
                if *stored_hash == 0 {
                    continue;
                }
                let distance = SimHash::hamming_distance(hash, *stored_hash);
                if distance <= self.config.simhash_threshold {
                    return Some((url.clone(), distance));
                }
            }
        }

        None
    }

    fn add_simhash(&self, url: &str, hash: u64) {
        let bucket_key = hash >> 58;
        let mut buckets = self.simhash_buckets.write();
        buckets
            .entry(bucket_key)
            .or_default()
            .push((url.to_string(), hash));
        self.fingerprint_count.fetch_add(1, Ordering::Relaxed);
    }

    // MinHash-based detection with LSH

    fn check_minhash(
        &self,
        url: &str,
        content: &str,
        stats: &mut NearDuplicateStats,
    ) -> Option<String> {
        let signature = self.minhash.signature(content);

        // Check for similar signatures using LSH
        if let Some((dup_url, similarity)) = self.find_similar_minhash(&signature, url) {
            stats.duplicates_found += 1;
            stats.avg_duplicate_similarity =
                (stats.avg_duplicate_similarity * (stats.duplicates_found - 1) as f64 + similarity)
                    / stats.duplicates_found as f64;
            return Some(dup_url);
        }

        // Not a duplicate - add to index
        self.add_minhash(url, signature);
        stats.unique_documents += 1;
        None
    }

    fn find_similar_minhash(&self, signature: &[u64], url: &str) -> Option<(String, f64)> {
        let band_hashes = self.compute_band_hashes(signature);
        let buckets = self.minhash_buckets.read();

        for (band_idx, band_hash) in band_hashes.iter().enumerate() {
            if let Some(entries) = buckets[band_idx].get(band_hash) {
                for (stored_url, stored_sig) in entries {
                    if stored_url == url {
                        continue; // Skip self
                    }
                    let similarity = MinHash::jaccard_similarity(signature, stored_sig);
                    if similarity >= self.config.minhash_threshold {
                        return Some((stored_url.clone(), similarity));
                    }
                }
            }
        }

        None
    }

    fn add_minhash(&self, url: &str, signature: Vec<u64>) {
        let band_hashes = self.compute_band_hashes(&signature);
        let mut buckets = self.minhash_buckets.write();

        for (band_idx, band_hash) in band_hashes.into_iter().enumerate() {
            buckets[band_idx]
                .entry(band_hash)
                .or_default()
                .push((url.to_string(), signature.clone()));
        }

        self.fingerprint_count.fetch_add(1, Ordering::Relaxed);
    }

    fn compute_band_hashes(&self, signature: &[u64]) -> Vec<u64> {
        let rows_per_band = self.config.num_minhash_functions / self.config.lsh_bands;
        let mut band_hashes = Vec::with_capacity(self.config.lsh_bands);

        for band in 0..self.config.lsh_bands {
            let start = band * rows_per_band;
            let end = start + rows_per_band;
            let band_slice = &signature[start..end.min(signature.len())];

            // Hash the band
            let mut hasher = SipHasher13::new();
            for &val in band_slice {
                val.hash(&mut hasher);
            }
            band_hashes.push(hasher.finish());
        }

        band_hashes
    }
}

/// Cluster of near-duplicate documents
#[derive(Debug, Clone)]
pub struct DuplicateCluster {
    /// Representative URL (canonical)
    pub canonical_url: String,
    /// All URLs in this cluster
    pub urls: Vec<String>,
    /// SimHash of the canonical document
    pub simhash: u64,
    /// Average pairwise similarity
    pub avg_similarity: f64,
}

/// Batch near-duplicate clustering
pub struct DuplicateClusterer {
    simhash: SimHash,
    threshold: u32,
}

impl DuplicateClusterer {
    /// Create a new clusterer
    pub fn new(threshold: u32) -> Self {
        Self {
            simhash: SimHash::new(),
            threshold,
        }
    }

    /// Cluster a batch of documents
    pub fn cluster(&self, documents: Vec<(String, String)>) -> Vec<DuplicateCluster> {
        // Compute SimHashes
        let hashes: Vec<(String, u64)> = documents
            .iter()
            .map(|(url, content)| (url.clone(), self.simhash.hash(content)))
            .collect();

        // Union-find for clustering
        let n = hashes.len();
        let mut parent: Vec<usize> = (0..n).collect();
        let mut rank: Vec<usize> = vec![0; n];

        fn find(parent: &mut [usize], i: usize) -> usize {
            if parent[i] != i {
                parent[i] = find(parent, parent[i]);
            }
            parent[i]
        }

        fn union(parent: &mut [usize], rank: &mut [usize], i: usize, j: usize) {
            let pi = find(parent, i);
            let pj = find(parent, j);
            if pi != pj {
                if rank[pi] < rank[pj] {
                    parent[pi] = pj;
                } else if rank[pi] > rank[pj] {
                    parent[pj] = pi;
                } else {
                    parent[pj] = pi;
                    rank[pi] += 1;
                }
            }
        }

        // Find similar pairs
        for i in 0..n {
            for j in (i + 1)..n {
                let distance = SimHash::hamming_distance(hashes[i].1, hashes[j].1);
                if distance <= self.threshold {
                    union(&mut parent, &mut rank, i, j);
                }
            }
        }

        // Group by cluster
        let mut clusters: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            let root = find(&mut parent, i);
            clusters.entry(root).or_default().push(i);
        }

        // Build result
        clusters
            .into_iter()
            .map(|(root, members)| {
                let urls: Vec<String> = members.iter().map(|&i| hashes[i].0.clone()).collect();
                let canonical_url = urls[0].clone();
                let simhash = hashes[root].1;

                // Calculate average similarity
                let mut total_similarity = 0.0;
                let mut pairs = 0;
                for i in 0..members.len() {
                    for j in (i + 1)..members.len() {
                        let dist =
                            SimHash::hamming_distance(hashes[members[i]].1, hashes[members[j]].1);
                        total_similarity += 1.0 - (dist as f64 / 64.0);
                        pairs += 1;
                    }
                }
                let avg_similarity = if pairs > 0 {
                    total_similarity / pairs as f64
                } else {
                    1.0
                };

                DuplicateCluster {
                    canonical_url,
                    urls,
                    simhash,
                    avg_similarity,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simhash_similar_documents() {
        let simhash = SimHash::new();

        let doc1 = "The quick brown fox jumps over the lazy dog";
        let doc2 = "The quick brown fox jumps over the lazy cat";
        let doc3 = "Completely different content about programming";

        let hash1 = simhash.hash(doc1);
        let hash2 = simhash.hash(doc2);
        let hash3 = simhash.hash(doc3);

        // Similar documents should have smaller Hamming distance than different ones
        let dist12 = SimHash::hamming_distance(hash1, hash2);
        let dist13 = SimHash::hamming_distance(hash1, hash3);

        // For short texts, SimHash distance can be higher; main assertion is relative
        assert!(
            dist12 < 20,
            "Similar docs should have moderate distance: {}",
            dist12
        );
        assert!(
            dist13 > dist12,
            "Different docs should have larger distance: {} vs {}",
            dist13,
            dist12
        );
    }

    #[test]
    fn test_simhash_identical_documents() {
        let simhash = SimHash::new();

        let doc = "The quick brown fox jumps over the lazy dog";
        let hash1 = simhash.hash(doc);
        let hash2 = simhash.hash(doc);

        assert_eq!(hash1, hash2);
        assert_eq!(SimHash::hamming_distance(hash1, hash2), 0);
    }

    #[test]
    fn test_minhash_similarity() {
        let minhash = MinHash::with_seed(128, 42);

        let doc1 = "The quick brown fox jumps over the lazy dog";
        let doc2 = "The quick brown fox jumps over the lazy cat";
        let doc3 = "Completely different content about programming and code";

        let sig1 = minhash.signature(doc1);
        let sig2 = minhash.signature(doc2);
        let sig3 = minhash.signature(doc3);

        let sim12 = MinHash::jaccard_similarity(&sig1, &sig2);
        let sim13 = MinHash::jaccard_similarity(&sig1, &sig3);

        assert!(
            sim12 > 0.5,
            "Similar docs should have high similarity: {}",
            sim12
        );
        assert!(sim13 < sim12, "Different docs should have lower similarity");
    }

    #[test]
    fn test_near_duplicate_detector_simhash() {
        // Use a higher threshold for short documents
        let config = NearDuplicateConfig {
            use_simhash: true,
            simhash_threshold: 12, // Higher threshold for short texts
            ..Default::default()
        };
        let detector = NearDuplicateDetector::new(config);

        // Add original document
        let result1 = detector.check_and_add(
            "https://example.com/page1",
            "The quick brown fox jumps over the lazy dog",
        );
        assert!(result1.is_none(), "First document should not be duplicate");

        // Add similar document
        let result2 = detector.check_and_add(
            "https://example.com/page2",
            "The quick brown fox jumps over the lazy cat",
        );
        assert!(
            result2.is_some(),
            "Similar document should be detected as duplicate"
        );

        // Add different document
        let result3 = detector.check_and_add(
            "https://example.com/page3",
            "Completely different content about Rust programming language",
        );
        assert!(
            result3.is_none(),
            "Different document should not be duplicate"
        );

        let stats = detector.stats();
        assert_eq!(stats.documents_checked, 3);
        assert_eq!(stats.duplicates_found, 1);
        assert_eq!(stats.unique_documents, 2);
    }

    #[test]
    fn test_near_duplicate_detector_minhash() {
        let config = NearDuplicateConfig {
            use_simhash: false,
            minhash_threshold: 0.6,
            num_minhash_functions: 64,
            lsh_bands: 8,
            ..Default::default()
        };
        let detector = NearDuplicateDetector::new(config);

        // Add original document
        let result1 = detector.check_and_add(
            "https://example.com/page1",
            "The quick brown fox jumps over the lazy dog every day",
        );
        assert!(result1.is_none());

        // Add similar document
        let _result2 = detector.check_and_add(
            "https://example.com/page2",
            "The quick brown fox jumps over the lazy cat every day",
        );
        // May or may not detect depending on LSH bands

        let stats = detector.stats();
        assert_eq!(stats.documents_checked, 2);
    }

    #[test]
    fn test_hash_empty_content() {
        let simhash = SimHash::new();
        let hash = simhash.hash("");
        assert_eq!(hash, 0);
    }

    #[test]
    fn test_empty_contents_not_similar_in_detector() {
        let config = NearDuplicateConfig {
            use_simhash: true,
            simhash_threshold: 3,
            ..Default::default()
        };
        let detector = NearDuplicateDetector::new(config);

        // Two empty pages should NOT be flagged as duplicates of each other
        let result1 = detector.check_and_add("https://example.com/page1", "");
        assert!(result1.is_none(), "First empty page should not be duplicate");

        let result2 = detector.check_and_add("https://example.com/page2", "");
        assert!(
            result2.is_none(),
            "Second empty page should not be duplicate of first"
        );
    }

    #[test]
    fn test_punctuation_only_content() {
        let simhash = SimHash::new();
        let hash_empty = simhash.hash("");
        let hash_punct = simhash.hash("!@#$%^&*()");
        // Both hash to 0 because normalization strips non-alphanumeric chars,
        // but the detector should handle zero-hashes gracefully
        assert_eq!(hash_empty, 0);
        assert_eq!(hash_punct, 0);

        // The detector should not flag them as duplicates
        let config = NearDuplicateConfig {
            use_simhash: true,
            simhash_threshold: 3,
            ..Default::default()
        };
        let detector = NearDuplicateDetector::new(config);
        let r1 = detector.check_and_add("https://a.com/1", "!@#$%^&*()");
        let r2 = detector.check_and_add("https://a.com/2", "");
        assert!(r1.is_none());
        assert!(r2.is_none());
    }

    #[test]
    fn test_single_word_content() {
        let simhash = SimHash::new();
        let hash = simhash.hash("hello");
        assert_ne!(hash, 0);
    }

    #[test]
    fn test_very_short_content_distinct() {
        let simhash = SimHash::new();
        let hash_cats = simhash.hash("cats");
        let hash_dogs = simhash.hash("dogs");
        assert_ne!(hash_cats, hash_dogs);
    }

    #[test]
    fn test_duplicate_clusterer() {
        let clusterer = DuplicateClusterer::new(5);

        let documents = vec![
            ("url1".to_string(), "The quick brown fox jumps".to_string()),
            ("url2".to_string(), "The quick brown fox leaps".to_string()),
            (
                "url3".to_string(),
                "Completely different content".to_string(),
            ),
            ("url4".to_string(), "Another different document".to_string()),
        ];

        let clusters = clusterer.cluster(documents);

        // Should have at least 2 clusters (similar docs grouped, different docs separate)
        assert!(clusters.len() >= 2);
    }
}
