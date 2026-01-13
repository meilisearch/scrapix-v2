# Scrapix Performance Analysis

## Test Results: Wikipedia 10k Crawl

**Test Date:** January 11, 2026
**Configuration:** Wikipedia crawl with 100 concurrent requests

### Metrics Collected

After ~20 minutes of crawling:

| Component | Metric | Value |
|-----------|--------|-------|
| **Crawler** | Pages Processed | 8,635 |
| | Pages Succeeded | 8,274 |
| | Pages Failed | 361 |
| | Data Downloaded | 1.4 GB |
| | Active Connections | 100 |
| | Rate | ~7 pages/second |
| **Frontier** | URLs Consumed | 2.4M |
| | New URLs | 354,325 |
| | Duplicates | 2.05M (85% duplicate rate) |
| | URLs Dispatched | 9,564 |
| | URLs Delayed | 4.7M |
| **Content** | Pages Processed | 3,796 |
| | Pages Indexed | 1,096 |
| | Data Processed | 812 MB |

### Identified Bottlenecks

#### 1. Frontier Rate Limiting (CRITICAL)

**Problem:** The frontier service was delaying 4.7M URLs while only dispatching 9.5k URLs.

**Root Cause:**
- `DOMAIN_DELAY_MS=1000` (1 second between same-domain requests)
- `CONCURRENT_PER_DOMAIN=2` (only 2 concurrent requests per domain)

For single-domain crawls like Wikipedia, this limits theoretical throughput to:
```
2 concurrent × (1000ms / 1000ms delay) = 2 pages/second
```

Even with the crawl config's 20 pages/second rate limit, the frontier was the bottleneck.

**Fix Applied:**
```rust
// Before
DOMAIN_DELAY_MS=1000
CONCURRENT_PER_DOMAIN=2
DISPATCH_BATCH_SIZE=100

// After
DOMAIN_DELAY_MS=100
CONCURRENT_PER_DOMAIN=20
DISPATCH_BATCH_SIZE=500
```

**Expected Improvement:**
```
20 concurrent × (1000ms / 100ms) = 200 pages/second theoretical max
```

#### 2. Content Worker Batching (MODERATE)

**Problem:** Only 1,096 documents indexed out of 3,796 processed (28% indexing rate).

**Root Cause:**
- `BATCH_SIZE=100` with 5-second flush intervals
- Low `CONCURRENCY=5` limited processing throughput

**Fix Applied:**
```rust
// Before
CONCURRENCY=5
BATCH_SIZE=100

// After
CONCURRENCY=20
BATCH_SIZE=500
```

**Expected Improvement:** 4x faster document processing, larger batches reduce Meilisearch overhead.

#### 3. URL Deduplication Overhead (MINOR)

**Observation:** 85% of discovered URLs were duplicates (Wikipedia has many cross-links).

**Current Behavior:** Bloom filter deduplication is working correctly. High duplicate rate is expected for heavily interlinked sites like Wikipedia.

**Recommendation:** No changes needed. Bloom filter efficiently handles this load.

### Performance Tuning Guide

#### For Single-Domain Crawls (Wikipedia, documentation sites)

```bash
# Aggressive settings for fast crawling
DOMAIN_DELAY_MS=50
CONCURRENT_PER_DOMAIN=50
DISPATCH_BATCH_SIZE=1000
CONCURRENCY=50        # crawler
CONCURRENCY=30        # content worker
BATCH_SIZE=1000
```

#### For Multi-Domain Crawls (general web crawling)

```bash
# Conservative settings to respect rate limits
DOMAIN_DELAY_MS=1000
CONCURRENT_PER_DOMAIN=5
DISPATCH_BATCH_SIZE=100
```

#### For Maximum Throughput Testing

```bash
# Use with caution - may trigger rate limits
DOMAIN_DELAY_MS=10
CONCURRENT_PER_DOMAIN=100
DISPATCH_BATCH_SIZE=2000
```

### Throughput Targets

Based on project goals from CLAUDE.md:

| Phase | Target | Pages/Second | Status |
|-------|--------|--------------|--------|
| MVP | 1M pages/day | 12 pages/sec | Achievable |
| Growth | 10M pages/day | 116 pages/sec | Needs tuning |
| Scale | 100M pages/day | 1,157 pages/sec | Requires k8s scaling |

### Recommended Test Commands

```bash
# Quick test (1k pages, ~30 seconds)
./scripts/scrapix-dev test wikipedia-1k -c 100 -d 20

# Medium test (10k pages, ~5-10 minutes)
./scripts/scrapix-dev test wikipedia-10k -c 150 -d 10

# Full benchmark
./scripts/scrapix-dev bench wikipedia
```

### Next Steps

1. **Run benchmarks with new defaults** to verify improvements
2. **Add Prometheus metrics endpoint** for real-time monitoring
3. **Implement adaptive rate limiting** based on server response codes
4. **Add connection pooling metrics** to identify HTTP layer bottlenecks
5. **Test with multiple crawler workers** for horizontal scaling

## Tooling Created

| Script | Purpose |
|--------|---------|
| `scripts/scrapix-dev` | Main CLI wrapper |
| `scripts/test-crawl.sh` | Run crawls with metrics |
| `scripts/bench.sh` | Run benchmarks |
| `scripts/k8s.sh` | Kubernetes management |
| `scripts/monitor.sh` | Real-time monitoring dashboard |

Usage:
```bash
./scripts/scrapix-dev test wikipedia-10k
./scripts/scrapix-dev bench
./scripts/scrapix-dev monitor
./scripts/scrapix-dev k8s deploy
```
