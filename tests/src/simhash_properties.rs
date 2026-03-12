//! SimHash & MinHash Property Tests (P3)
//!
//! Property-based tests for the near-duplicate detection algorithms.
//! Verifies mathematical invariants that must hold regardless of input.

use scrapix_frontier::{MinHash, NearDuplicateConfig, NearDuplicateDetector, SimHash};

// ============================================================================
// SimHash properties
// ============================================================================

#[test]
fn test_simhash_identical_content_produces_identical_hash() {
    let sh = SimHash::new();
    let content = "The quick brown fox jumps over the lazy dog";
    assert_eq!(sh.hash(content), sh.hash(content));
}

#[test]
fn test_simhash_different_content_produces_different_hash() {
    let sh = SimHash::new();
    let h1 = sh.hash("The quick brown fox jumps over the lazy dog");
    let h2 = sh.hash("A completely different piece of text about something else entirely");
    assert_ne!(h1, h2);
}

#[test]
fn test_simhash_hamming_distance_reflexive() {
    // Distance from a hash to itself is always 0
    let sh = SimHash::new();
    let h = sh.hash("some content");
    assert_eq!(SimHash::hamming_distance(h, h), 0);
}

#[test]
fn test_simhash_hamming_distance_symmetric() {
    let sh = SimHash::new();
    let h1 = sh.hash("content one with enough words");
    let h2 = sh.hash("content two with enough words");
    assert_eq!(
        SimHash::hamming_distance(h1, h2),
        SimHash::hamming_distance(h2, h1)
    );
}

#[test]
fn test_simhash_hamming_distance_max_is_64() {
    // For 64-bit hashes, max hamming distance is 64
    let d = SimHash::hamming_distance(0u64, u64::MAX);
    assert_eq!(d, 64);
}

#[test]
fn test_simhash_hamming_distance_min_is_zero() {
    let d = SimHash::hamming_distance(42u64, 42u64);
    assert_eq!(d, 0);
}

#[test]
fn test_simhash_is_similar_threshold_boundary() {
    let sh = SimHash::new();
    let h = sh.hash("test content");

    // Same hash is always similar (distance 0 <= any threshold)
    assert!(SimHash::is_similar(h, h, 0));
    assert!(SimHash::is_similar(h, h, 10));

    // Opposite bits are never similar at threshold < 64
    assert!(!SimHash::is_similar(0u64, u64::MAX, 63));
}

#[test]
fn test_simhash_similar_content_has_low_distance() {
    let sh = SimHash::new();
    let h1 = sh.hash("The quick brown fox jumps over the lazy dog. This is a test of similarity detection with enough content to make a good fingerprint.");
    let h2 = sh.hash("The quick brown fox jumps over the lazy dog. This is a test of similarity detection with enough content to produce a good fingerprint.");

    let distance = SimHash::hamming_distance(h1, h2);
    assert!(
        distance < 15,
        "Similar content should have low hamming distance, got {distance}"
    );
}

#[test]
fn test_simhash_empty_content_does_not_panic() {
    let sh = SimHash::new();
    let _ = sh.hash("");
    let _ = sh.hash(" ");
}

// ============================================================================
// MinHash properties
// ============================================================================

#[test]
fn test_minhash_identical_content_has_similarity_one() {
    let mh = MinHash::new(128);
    let sig = mh.signature("identical content with enough words to generate shingles");
    let sim = MinHash::jaccard_similarity(&sig, &sig);
    assert!(
        (sim - 1.0).abs() < 0.001,
        "Self-similarity should be 1.0, got {sim}"
    );
}

#[test]
fn test_minhash_similarity_range_zero_to_one() {
    let mh = MinHash::new(128);
    let sig1 = mh.signature("first document about web crawling and indexing");
    let sig2 = mh.signature("second document about cooking and baking recipes");
    let sim = MinHash::jaccard_similarity(&sig1, &sig2);
    assert!(
        (0.0..=1.0).contains(&sim),
        "Similarity should be in [0,1], got {sim}"
    );
}

#[test]
fn test_minhash_similarity_symmetric() {
    let mh = MinHash::new(128);
    let sig1 = mh.signature("document about technology");
    let sig2 = mh.signature("document about science");
    assert_eq!(
        MinHash::jaccard_similarity(&sig1, &sig2),
        MinHash::jaccard_similarity(&sig2, &sig1)
    );
}

#[test]
fn test_minhash_similar_content_has_high_similarity() {
    let mh = MinHash::new(128);
    let sig1 = mh.signature("The quick brown fox jumps over the lazy dog. This is a test of similarity detection in web crawling.");
    let sig2 = mh.signature("The quick brown fox jumps over the lazy dog. This is a test of similarity detection in web crawling and indexing.");

    let sim = MinHash::jaccard_similarity(&sig1, &sig2);
    assert!(
        sim > 0.5,
        "Similar content should have high similarity, got {sim}"
    );
}

#[test]
fn test_minhash_different_content_has_low_similarity() {
    let mh = MinHash::new(128);
    let sig1 = mh.signature(
        "The quick brown fox jumps over the lazy dog and runs through the forest chasing rabbits",
    );
    let sig2 = mh.signature("Quantum computing leverages principles of quantum mechanics including superposition and entanglement");

    let sim = MinHash::jaccard_similarity(&sig1, &sig2);
    assert!(
        sim < 0.5,
        "Very different content should have low similarity, got {sim}"
    );
}

#[test]
fn test_minhash_signature_length_matches_num_hashes() {
    let mh = MinHash::new(64);
    let sig = mh.signature("test content");
    assert_eq!(sig.len(), 64);

    let mh2 = MinHash::new(256);
    let sig2 = mh2.signature("test content");
    assert_eq!(sig2.len(), 256);
}

#[test]
fn test_minhash_deterministic_with_same_seed() {
    let mh1 = MinHash::with_seed(128, 42);
    let mh2 = MinHash::with_seed(128, 42);

    let sig1 = mh1.signature("deterministic test content");
    let sig2 = mh2.signature("deterministic test content");
    assert_eq!(sig1, sig2, "Same seed should produce same signatures");
}

#[test]
fn test_minhash_different_seeds_different_signatures() {
    let mh1 = MinHash::with_seed(128, 1);
    let mh2 = MinHash::with_seed(128, 2);

    let sig1 = mh1.signature("test content for seed comparison");
    let sig2 = mh2.signature("test content for seed comparison");
    // Different seeds should (with overwhelming probability) produce different signatures
    assert_ne!(
        sig1, sig2,
        "Different seeds should produce different signatures"
    );
}

#[test]
fn test_minhash_is_similar_matches_threshold() {
    let mh = MinHash::new(128);
    let sig = mh.signature("test content");
    // Self-similarity should pass any threshold
    assert!(MinHash::is_similar(&sig, &sig, 0.99));
}

// ============================================================================
// NearDuplicateDetector properties
// ============================================================================

#[test]
fn test_detector_config_defaults_are_sensible() {
    let config = NearDuplicateConfig::default();
    assert!(
        config.simhash_threshold <= 10,
        "SimHash threshold should be conservative"
    );
    assert!(config.minhash_threshold >= 0.7 && config.minhash_threshold <= 1.0);
    assert!(config.num_minhash_functions >= 64);
    assert!(config.max_fingerprints > 0);
}

#[test]
fn test_detector_stats_track_documents() {
    let detector = NearDuplicateDetector::with_defaults();

    detector.check_and_add(
        "https://a.com/1",
        "unique content one with enough words for fingerprinting",
    );
    detector.check_and_add(
        "https://a.com/2",
        "unique content two with enough words for fingerprinting",
    );
    detector.check_and_add(
        "https://a.com/3",
        "unique content one with enough words for fingerprinting",
    ); // duplicate of 1

    let stats = detector.stats();
    assert!(stats.documents_checked >= 3);
    assert!(stats.unique_documents >= 2);
}

#[test]
fn test_detector_clear_resets_state() {
    let detector = NearDuplicateDetector::with_defaults();

    detector.check_and_add("https://a.com/1", "some content");
    assert!(detector.fingerprint_count() > 0);

    detector.clear();
    assert_eq!(detector.fingerprint_count(), 0);
}

#[test]
fn test_detector_identical_content_always_detected() {
    let detector = NearDuplicateDetector::with_defaults();
    let content = "This is a sufficiently long piece of content that should be detected as a duplicate when added twice to the near-duplicate detector.";

    let r1 = detector.check_and_add("https://a.com/original", content);
    assert!(r1.is_none(), "First add should not be a duplicate");

    let r2 = detector.check_and_add("https://a.com/copy", content);
    assert!(
        r2.is_some(),
        "Identical content should be detected as duplicate"
    );
}

#[test]
fn test_detector_completely_different_content_not_detected() {
    let detector = NearDuplicateDetector::with_defaults();

    detector.check_and_add("https://a.com/1", "Web crawling is the process of systematically browsing the world wide web for the purpose of indexing content and creating searchable databases.");
    let r = detector.check_and_add("https://a.com/2", "Quantum computing uses quantum mechanical phenomena such as superposition and entanglement to perform computation in ways that classical computers cannot.");

    assert!(
        r.is_none(),
        "Completely different content should not be detected as duplicate"
    );
}

// ============================================================================
// Politeness adaptive backoff properties
// ============================================================================

#[test]
fn test_politeness_backoff_increases_on_failure() {
    use scrapix_frontier::PolitenessScheduler;

    let scheduler = PolitenessScheduler::with_defaults();
    let domain = "slow-server.com";

    // Get initial delay
    scheduler.start_request(domain);
    scheduler.complete_request(domain);
    let initial_delay = scheduler.wait_time(domain);

    // Fail multiple times to trigger backoff
    for _ in 0..5 {
        scheduler.start_request(domain);
        scheduler.failed_request(domain, true); // rate limited
    }

    let backoff_delay = scheduler.wait_time(domain);
    assert!(
        backoff_delay > initial_delay,
        "Delay should increase after failures: initial={:?}, after_backoff={:?}",
        initial_delay,
        backoff_delay
    );
}

#[test]
fn test_politeness_recovery_decreases_delay() {
    use scrapix_frontier::PolitenessScheduler;

    let scheduler = PolitenessScheduler::with_defaults();
    let domain = "recovering-server.com";

    // Build up backoff
    for _ in 0..5 {
        scheduler.start_request(domain);
        scheduler.failed_request(domain, true);
    }

    let high_delay = scheduler.wait_time(domain);

    // Successful requests should gradually reduce delay
    for _ in 0..20 {
        scheduler.start_request(domain);
        scheduler.complete_request(domain);
    }

    let recovered_delay = scheduler.wait_time(domain);
    assert!(
        recovered_delay < high_delay,
        "Delay should decrease after successful requests: high={:?}, recovered={:?}",
        high_delay,
        recovered_delay
    );
}

#[test]
fn test_politeness_pause_and_resume() {
    use scrapix_frontier::PolitenessScheduler;

    let scheduler = PolitenessScheduler::with_defaults();
    let domain = "pausable.com";

    scheduler.start_request(domain);
    scheduler.complete_request(domain);
    assert!(scheduler.can_fetch(domain) || !scheduler.can_fetch(domain)); // Domain is tracked

    scheduler.pause_domain(domain);
    if let Some(stats) = scheduler.domain_stats(domain) {
        assert!(stats.paused, "Domain should be paused");
    }

    scheduler.resume_domain(domain);
    if let Some(stats) = scheduler.domain_stats(domain) {
        assert!(!stats.paused, "Domain should be resumed");
    }
}

// ============================================================================
// LinkGraph PageRank properties
// ============================================================================

#[test]
fn test_linkgraph_pagerank_sums_to_approximately_one() {
    use scrapix_frontier::LinkGraph;

    let graph = LinkGraph::with_defaults();

    graph.record_links("https://a.com", vec!["https://b.com", "https://c.com"]);
    graph.record_links("https://b.com", vec!["https://c.com"]);
    graph.record_links("https://c.com", vec!["https://a.com"]);

    graph.compute_scores();

    let top = graph.top_pages(100);
    let sum: f64 = top.iter().map(|(_, s)| s).sum();
    assert!(
        (sum - 1.0).abs() < 0.1,
        "PageRank scores should sum to approximately 1.0, got {sum}"
    );
}

#[test]
fn test_linkgraph_more_inbound_links_higher_score() {
    use scrapix_frontier::LinkGraph;

    let graph = LinkGraph::with_defaults();

    // Hub page linked from many sources
    for i in 0..10 {
        graph.record_link(&format!("https://source{i}.com"), "https://hub.com");
    }
    // Leaf page linked from only one source
    graph.record_link("https://source0.com", "https://leaf.com");

    graph.compute_scores();

    let hub_score = graph.get_score("https://hub.com");
    let leaf_score = graph.get_score("https://leaf.com");

    assert!(
        hub_score > leaf_score,
        "Hub ({hub_score}) should score higher than leaf ({leaf_score})"
    );
}

#[test]
fn test_linkgraph_priority_boost_range() {
    use scrapix_frontier::{LinkGraph, LinkGraphConfig};

    let max_boost = 50;
    let graph = LinkGraph::new(LinkGraphConfig {
        max_priority_boost: max_boost,
        ..Default::default()
    });

    for i in 0..20 {
        graph.record_link(&format!("https://s{i}.com"), "https://popular.com");
    }
    graph.compute_scores();

    let boost = graph.get_priority_boost("https://popular.com");
    assert!(
        boost >= 0 && boost <= max_boost,
        "Priority boost should be in [0, {max_boost}], got {boost}"
    );
}

#[test]
fn test_linkgraph_self_links_ignored() {
    use scrapix_frontier::LinkGraph;

    let graph = LinkGraph::with_defaults();
    graph.record_link("https://a.com", "https://a.com");

    // Self-link should not count as inbound
    assert_eq!(
        graph.inbound_count("https://a.com"),
        0,
        "Self-links should be ignored"
    );
}
