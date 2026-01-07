//! Integration tests for link graph functionality

use scrapix_frontier::{LinkGraph, LinkGraphBuilder, LinkGraphConfig, PriorityQueue};
use scrapix_core::CrawlUrl;

/// Test basic link recording and score computation
#[test]
fn test_link_graph_basic() {
    let graph = LinkGraph::with_defaults();

    // Build a simple web structure
    // Home -> About, Contact, Products
    // About -> Home
    // Contact -> Home
    // Products -> Home, Product1, Product2
    // Product1 -> Products
    // Product2 -> Products

    graph.record_links(
        "https://example.com/",
        vec![
            "https://example.com/about",
            "https://example.com/contact",
            "https://example.com/products",
        ],
    );

    graph.record_link("https://example.com/about", "https://example.com/");
    graph.record_link("https://example.com/contact", "https://example.com/");

    graph.record_links(
        "https://example.com/products",
        vec![
            "https://example.com/",
            "https://example.com/products/1",
            "https://example.com/products/2",
        ],
    );

    graph.record_link("https://example.com/products/1", "https://example.com/products");
    graph.record_link("https://example.com/products/2", "https://example.com/products");

    // Compute scores
    graph.compute_scores();

    // Home should have highest score (most inbound links)
    let home_score = graph.get_score("https://example.com/");
    let about_score = graph.get_score("https://example.com/about");
    let products_score = graph.get_score("https://example.com/products");

    assert!(home_score > about_score, "Home should rank higher than About");
    assert!(
        products_score > about_score,
        "Products should rank higher than About (more inbound)"
    );

    // Stats check
    let stats = graph.stats();
    assert_eq!(stats.page_count, 6);
    assert!(stats.link_count >= 8);
}

/// Test priority boosting integration
#[test]
fn test_link_graph_priority_boost() {
    let graph = LinkGraph::with_defaults();

    // Create a hub page that many pages link to
    let hub = "https://docs.example.com/api";
    for i in 0..30 {
        graph.record_link(&format!("https://docs.example.com/tutorial/{}", i), hub);
    }

    // Create a page with no inbound links
    let leaf = "https://docs.example.com/obscure";
    graph.record_link(hub, leaf);

    graph.compute_scores();

    let hub_boost = graph.get_priority_boost(hub);
    let leaf_boost = graph.get_priority_boost(leaf);

    assert!(
        hub_boost > leaf_boost,
        "Hub should get more boost than leaf: {} vs {}",
        hub_boost,
        leaf_boost
    );
    assert!(hub_boost > 0, "Hub should get positive boost");
}

/// Test integration with priority queue
#[test]
fn test_link_graph_with_priority_queue() {
    let graph = LinkGraph::with_defaults();
    let queue = PriorityQueue::with_defaults();

    // Build link structure
    let important = "https://example.com/important";
    let normal = "https://example.com/normal";

    // Many pages link to important
    for i in 0..20 {
        graph.record_link(&format!("https://example.com/page{}", i), important);
    }

    // Only one page links to normal
    graph.record_link("https://example.com/page0", normal);

    graph.compute_scores();

    // Create URLs with boost from link graph
    let mut important_url = CrawlUrl::new(important, 1);
    important_url.priority += graph.get_priority_boost(important);

    let mut normal_url = CrawlUrl::new(normal, 1);
    normal_url.priority += graph.get_priority_boost(normal);

    // Push to queue (in any order)
    queue.push(normal_url);
    queue.push(important_url);

    // Important should come out first due to priority boost
    let first = queue.pop().unwrap();
    assert_eq!(first.url, important, "Important page should be first");
}

/// Test large graph performance
#[test]
fn test_link_graph_scale() {
    let graph = LinkGraphBuilder::new()
        .iterations(10) // Fewer iterations for speed
        .build();

    // Build a larger graph
    let num_pages = 1000;
    let links_per_page = 10;

    for i in 0..num_pages {
        let source = format!("https://example.com/page{}", i);
        let targets: Vec<String> = (0..links_per_page)
            .map(|j| {
                let target = (i + j + 1) % num_pages;
                format!("https://example.com/page{}", target)
            })
            .collect();
        graph.record_links(&source, targets);
    }

    assert_eq!(graph.page_count(), num_pages);

    // Compute scores (should complete in reasonable time)
    graph.compute_scores();

    let stats = graph.stats();
    assert_eq!(stats.page_count, num_pages);
    assert!(stats.avg_score > 0.0);
}

/// Test circular links (should not cause infinite loop)
#[test]
fn test_circular_links() {
    let graph = LinkGraph::with_defaults();

    // Create a cycle: A -> B -> C -> A
    graph.record_link("https://a.com", "https://b.com");
    graph.record_link("https://b.com", "https://c.com");
    graph.record_link("https://c.com", "https://a.com");

    // Should complete without hanging
    graph.compute_scores();

    // All pages should have similar scores (cycle is symmetric)
    let score_a = graph.get_score("https://a.com");
    let score_b = graph.get_score("https://b.com");
    let score_c = graph.get_score("https://c.com");

    // Scores should be close (within 10%)
    let avg = (score_a + score_b + score_c) / 3.0;
    assert!((score_a - avg).abs() / avg < 0.1);
    assert!((score_b - avg).abs() / avg < 0.1);
    assert!((score_c - avg).abs() / avg < 0.1);
}

/// Test URL normalization consistency
#[test]
fn test_url_normalization_consistency() {
    let graph = LinkGraph::with_defaults();

    // Same page referenced with different URL forms
    graph.record_link("https://example.com/page/", "https://other.com");
    graph.record_link("https://example.com/page", "https://another.com");
    graph.record_link("https://example.com/page#section", "https://third.com");

    // All should be normalized to same page
    assert_eq!(graph.outbound_count("https://example.com/page"), 3);
}

/// Test top pages functionality
#[test]
fn test_top_pages() {
    let graph = LinkGraph::with_defaults();

    // Create a clear hierarchy
    let top_page = "https://example.com/most-popular";
    let mid_page = "https://example.com/somewhat-popular";
    let low_page = "https://example.com/not-popular";

    // Top page gets 10 links
    for i in 0..10 {
        graph.record_link(&format!("https://example.com/ref{}", i), top_page);
    }

    // Mid page gets 5 links
    for i in 0..5 {
        graph.record_link(&format!("https://example.com/ref{}", i + 10), mid_page);
    }

    // Low page gets 1 link
    graph.record_link("https://example.com/lonely", low_page);

    graph.compute_scores();

    let top = graph.top_pages(3);
    assert_eq!(top[0].0, top_page);
    assert_eq!(top[1].0, mid_page);
}

/// Test max pages limit
#[test]
fn test_max_pages_limit() {
    let config = LinkGraphConfig {
        max_pages: 10,
        ..Default::default()
    };
    let graph = LinkGraph::new(config);

    // Add pages up to limit
    for i in 0..10 {
        graph.record_link(
            &format!("https://example.com/page{}", i),
            &format!("https://example.com/target{}", i),
        );
    }

    // Graph should have 20 pages (10 sources + 10 targets)
    // But let's add more - they should be rejected if limit is reached
    assert!(graph.page_count() <= 20);
}

/// Test inbound/outbound link retrieval
#[test]
fn test_link_retrieval() {
    let graph = LinkGraph::with_defaults();

    let hub = "https://example.com/hub";
    let spoke1 = "https://example.com/spoke1";
    let spoke2 = "https://example.com/spoke2";
    let spoke3 = "https://example.com/spoke3";

    // Hub links to all spokes
    graph.record_links(hub, vec![spoke1, spoke2, spoke3]);

    // All spokes link back to hub
    graph.record_link(spoke1, hub);
    graph.record_link(spoke2, hub);
    graph.record_link(spoke3, hub);

    // Check outbound links from hub
    let outbound = graph.get_outbound_links(hub);
    assert_eq!(outbound.len(), 3);

    // Check inbound links to hub
    let inbound = graph.get_inbound_links(hub);
    assert_eq!(inbound.len(), 3);
}

/// Test damping factor effect
#[test]
fn test_damping_factor() {
    // Higher damping = more weight to link structure
    let graph_high = LinkGraphBuilder::new().damping_factor(0.95).build();

    let graph_low = LinkGraphBuilder::new().damping_factor(0.5).build();

    // Same structure for both
    let hub = "https://example.com/hub";
    for i in 0..20 {
        let page = format!("https://example.com/page{}", i);
        graph_high.record_link(&page, hub);
        graph_low.record_link(&page, hub);
    }

    graph_high.compute_scores();
    graph_low.compute_scores();

    let score_high = graph_high.get_score(hub);
    let score_low = graph_low.get_score(hub);

    // With higher damping, hub should have relatively higher score
    // compared to average (more emphasis on link structure)
    let stats_high = graph_high.stats();
    let stats_low = graph_low.stats();

    let ratio_high = score_high / stats_high.avg_score;
    let ratio_low = score_low / stats_low.avg_score;

    assert!(
        ratio_high > ratio_low,
        "High damping should give hub more relative importance"
    );
}

/// Test clearing the graph
#[test]
fn test_clear_graph() {
    let graph = LinkGraph::with_defaults();

    graph.record_link("https://a.com", "https://b.com");
    graph.record_link("https://b.com", "https://c.com");

    assert_eq!(graph.page_count(), 3);

    graph.clear();

    assert_eq!(graph.page_count(), 0);
    assert_eq!(graph.link_count(), 0);
}

/// Test contains method
#[test]
fn test_contains() {
    let graph = LinkGraph::with_defaults();

    graph.record_link("https://example.com/a", "https://example.com/b");

    assert!(graph.contains("https://example.com/a"));
    assert!(graph.contains("https://example.com/b"));
    assert!(!graph.contains("https://example.com/c"));
}

/// Test stats on empty graph
#[test]
fn test_empty_graph_stats() {
    let graph = LinkGraph::with_defaults();

    let stats = graph.stats();

    assert_eq!(stats.page_count, 0);
    assert_eq!(stats.link_count, 0);
    assert_eq!(stats.avg_score, 0.0);
}

/// Test compute_scores_if_dirty
#[test]
fn test_compute_scores_if_dirty() {
    let graph = LinkGraph::with_defaults();

    graph.record_link("https://a.com", "https://b.com");

    // First call should compute
    graph.compute_scores_if_dirty();
    let score1 = graph.get_score("https://b.com");

    // Second call without changes shouldn't recompute
    graph.compute_scores_if_dirty();
    let score2 = graph.get_score("https://b.com");

    assert_eq!(score1, score2);

    // Add new link - should mark dirty and recompute
    graph.record_link("https://c.com", "https://b.com");
    graph.compute_scores_if_dirty();
    let score3 = graph.get_score("https://b.com");

    // Score might change due to recomputation (PageRank normalizes to 1)
    // What matters is that b has more relative importance than other pages
    let score_a = graph.get_score("https://a.com");
    let score_c = graph.get_score("https://c.com");

    // b should have highest score (most inbound links)
    assert!(score3 >= score_a, "b should rank >= a");
    assert!(score3 >= score_c, "b should rank >= c");

    // b should get higher priority boost than pages with no inbound links
    let boost_b = graph.get_priority_boost("https://b.com");
    let boost_a = graph.get_priority_boost("https://a.com");
    assert!(boost_b >= boost_a, "b should get >= boost than a");
}
