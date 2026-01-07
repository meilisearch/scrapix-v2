# Scrapix v2: Rust-Based Internet-Scale Crawler

## Vision

Build a high-performance, distributed web crawler capable of:
1. **Global Internet Indexing** - Crawl billions of pages like Google
2. **Targeted Site Crawling** - Index specific websites/documentation
3. **Real-time Information Retrieval** - On-demand crawling like [Exa.ai](https://exa.ai)

---

## Technology Stack Decisions

### Core Language: Rust

**Why Rust for everything:**
- **Zero-cost abstractions** - No garbage collection pauses
- **Memory safety** - No segfaults, no data races
- **Async/await** - Native async with Tokio runtime
- **Performance** - C/C++ level performance
- **Strong ecosystem** - reqwest, tokio, scraper, etc.

### Message Queue: Redpanda

**Decision**: [Redpanda](https://redpanda.com) over Kafka/Pulsar/NATS

| Criteria | Redpanda | Kafka | Pulsar | NATS |
|----------|----------|-------|--------|------|
| **Performance** | 10x lower latency | Baseline | 2.5x faster | Ultra-low |
| **Language** | C++/Rust | Java | Java | Go |
| **Complexity** | Single binary | Complex | Very complex | Simple |
| **Kafka API** | 100% compatible | Native | Partial | No |
| **Resources** | 3-6x less | Baseline | Similar | Minimal |

**Why Redpanda:**
- [10x faster tail latencies](https://www.redpanda.com/blog/redpanda-vs-kafka-performance-benchmark) than Kafka
- [3-6x fewer compute resources](https://www.redpanda.com/compare/redpanda-vs-kafka)
- No ZooKeeper dependency - single binary
- 100% Kafka API compatible - use any Kafka client
- Written in C++ with thread-per-core architecture
- [Rust client fully supported](https://docs.redpanda.com/current/develop/kafka-clients/)

**Use Cases in Our System:**
- URL frontier (partitioned by domain)
- Event streaming (crawl events, metrics)
- Task distribution to workers

### Search Engine: Meilisearch

**Decision**: Keep [Meilisearch](https://meilisearch.com)

| Criteria | Meilisearch | Typesense | Elasticsearch |
|----------|-------------|-----------|---------------|
| **Language** | Rust | C++ | Java |
| **Latency** | <50ms | <50ms | 100-500ms |
| **Setup** | Minutes | Minutes | Days/Weeks |
| **Index Size** | Up to 80TiB | RAM-limited | Petabytes |
| **Best For** | End-user search | End-user search | Log analytics |

**Why Meilisearch:**
- **Rust-based** - aligns with our stack
- [Memory-mapped storage](https://www.meilisearch.com/blog/meilisearch-vs-typesense) handles large datasets
- Sub-50ms search latency
- Built-in typo tolerance, faceting, filtering
- Simple API, excellent developer experience
- [Better multi-language support](https://typesense.org/typesense-vs-algolia-vs-elasticsearch-vs-meilisearch/) than Typesense

**For Analytics/Logs**: Consider ClickHouse as secondary store

### State Storage: Meilisearch-Centric + RocksDB

**Decision**: Simplified architecture with Meilisearch as primary store

| Component | Technology | Purpose |
|-----------|------------|---------|
| **Search + Metadata + Vectors** | Meilisearch | Documents, embeddings, crawl metadata |
| **Local State** | RocksDB | Per-worker URL cache, robots.txt, DNS |
| **Hot Cache** | Redis/DragonflyDB | Rate limiting, real-time counters |

**Why Meilisearch for Everything:**
- Already handles full-text search
- Native vector search support (embeddings)
- Filterable attributes for metadata queries
- Reduces operational complexity (one less database)
- Rust-native, aligns with our stack

**RocksDB** (embedded):
- [High-performance LSM-tree](https://db-engines.com/en/system/Redis%3BRocksDB%3BScyllaDB)
- Perfect for local crawler state
- Rust bindings via `rocksdb` crate
- Used by: Facebook, Netflix, Uber

**DragonflyDB** (cache):
- Redis-compatible, [80% lower costs](https://slashdot.org/software/comparison/Redis-vs-RocksDB-vs-ScyllaDB/)
- Multi-threaded (unlike Redis)
- In-memory with persistence
- Rate limiting per domain

### URL Deduplication: Bloom Filters

**Decision**: [Bloom filters](https://ieeexplore.ieee.org/document/7809713/) for probabilistic dedup

**Why Bloom Filters:**
- [90% less memory](https://grokkingthesystemdesign.com/guides/web-crawler-system-design/) vs hash sets (1.2GB vs 12GB for 1B URLs)
- O(1) lookup time
- False positives acceptable (URL skipped, will be seen again)
- No false negatives (never miss a new URL)

**Implementation:**
- Use `bloomfilter` or `probabilistic-collections` Rust crates
- Jenkins + Murmur hash combination
- Partition bloom filters by domain hash
- Periodic compaction/rebuild

### Near-Duplicate Detection: SimHash + MinHash

**Decision**: Dual locality-sensitive hashing for content deduplication

**Why Both Methods:**

| Method | Strength | Use Case |
|--------|----------|----------|
| **SimHash** | Fast 64-bit fingerprints | Quick similarity check |
| **MinHash** | Accurate Jaccard estimation | Fine-grained clustering |

**SimHash Implementation:**
- 64-bit fingerprint via weighted token hashing
- Hamming distance for similarity (threshold ~10 bits)
- LSH buckets for O(1) candidate lookup
- SipHash-based consistent hashing

**MinHash Implementation:**
- 128 hash functions for ~2% error margin
- Jaccard similarity estimation
- Works on n-gram shingles (3-5 tokens)
- Threshold typically 0.8 for duplicates

**Integration:**
```
Content → SimHash → LSH Bucket Lookup → MinHash Verification → Dedupe Decision
```

### Analytics: ClickHouse

**Decision**: ClickHouse for crawl analytics and aggregations

**Why ClickHouse:**
- Columnar storage optimized for analytics
- 100x faster than PostgreSQL for aggregations
- Excellent compression (10-20x)
- SQL interface, easy integration

**Tables:**
- `crawl_events` - Per-page fetch telemetry
- `content_events` - Processed content metadata
- Materialized views for hourly/daily aggregates

### Monitoring: Prometheus + Grafana

**Decision**: Standard observability stack

**Components:**
- **Prometheus** - Metrics collection and alerting
- **Grafana** - Visualization dashboards
- **Alertmanager** - Alert routing and deduplication

**Key Metrics:**
- Crawl rate (pages/sec by domain, worker)
- Error rate (by type: timeout, 4xx, 5xx, parse)
- Latency percentiles (p50, p95, p99)
- Queue depths (frontier, content processing)
- Resource utilization (CPU, memory, network)

### Crawler Stack: Custom Rust

**Core Libraries:**

| Library | Purpose | Crate |
|---------|---------|-------|
| **HTTP Client** | Fetching pages | `reqwest` |
| **Async Runtime** | Concurrency | `tokio` |
| **HTML Parsing** | DOM extraction | `scraper` |
| **URL Handling** | Normalization | `url` |
| **Robots.txt** | Politeness | `robotstxt` |
| **DNS** | Resolution | `trust-dns` |

**Why Custom vs Spider-rs:**
- [Spider-rs](https://github.com/spider-rs/spider) is good but monolithic
- Custom gives us control over distributed architecture
- Can optimize for our specific use case (frontier, dedup, etc.)

### JavaScript Rendering

**Decision**: Use `chromiumoxide` for JS-rendered pages.

Most pages (80-90%) don't need JS rendering and work with HTTP + `scraper`. For JS-heavy pages, use a headless Chrome/Chromium via `chromiumoxide`.

#### Future options
- [Lightpanda](https://lightpanda.io) (CDP-compatible, fast startup) — consider when mature.
- [Servo](https://servo.org) (Rust-native engine) — evaluate when production-ready.

#### Implementation Plan

**Phase 1 (MVP)**: HTTP-only with reqwest + scraper
**Phase 2**: Add chromiumoxide for JS-heavy sites
**Phase 3**: Integrate Lightpanda when stable (watch releases)
**Phase 4**: Evaluate Servo when production-ready

### Object Storage: RustFS

**Decision**: [RustFS](https://github.com/rustfs/rustfs) - S3-compatible object storage in Rust

| Use Case | Storage |
|----------|---------|
| Raw HTML archive | RustFS |
| Rendered screenshots | RustFS |
| Crawl logs | RustFS + ClickHouse |
| Config/state backup | RustFS |

**Why RustFS over MinIO/S3:**
- **Rust-native** - aligns with our stack
- **100% S3 compatible** - works with any S3 client
- **High performance** - designed for data lakes, AI workloads
- **Self-hosted** - no cloud dependency
- **Memory safe** - Rust guarantees

**Deployment:**
- Port 9000: S3 API endpoint
- Port 9001: Web console
- Supports distributed mode for scale

---

## System Architecture

### High-Level Overview

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                   API LAYER                                     │
├─────────────────────────────────────────────────────────────────────────────────┤
│  REST API (axum)  │  WebSocket                                                  │
└─────────────────────────────────────────────────────────────────────────────────┘
                                        │
                                        ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                              ORCHESTRATION LAYER                                │
├─────────────────────────────────────────────────────────────────────────────────┤
│  Job Scheduler  │  Rate Limiter  │  Priority Manager  │  Config Service         │
└─────────────────────────────────────────────────────────────────────────────────┘
                                        │
              ┌─────────────────────────┼─────────────────────────┐
              ▼                         ▼                         ▼
┌───────────────────────┐  ┌───────────────────────┐  ┌───────────────────────┐
│    URL FRONTIER       │  │   CRAWLER WORKERS     │  │   CONTENT WORKERS     │
│    (Redpanda)         │  │   (Rust binaries)     │  │   (Rust binaries)     │
├───────────────────────┤  ├───────────────────────┤  ├───────────────────────┤
│ • Priority queues     │  │ • HTTP fetching       │  │ • HTML parsing        │
│ • Domain partitioning │  │ • JS rendering pool   │  │ • Feature extraction  │
│ • Politeness delays   │  │ • Proxy rotation      │  │ • AI enrichment       │
│ • Bloom filter dedup  │  │ • DNS caching         │  │ • Document building   │
│ • Depth tracking      │  │ • robots.txt          │  │ • Schema extraction   │
└───────────────────────┘  └───────────────────────┘  └───────────────────────┘
              │                         │                         │
              ▼                         ▼                         ▼
┌─────────────────────────────────────────────────────────────────────────────────┐
│                                 DATA LAYER                                      │
├─────────────────────────────────────────────────────────────────────────────────┤
│ Redpanda     │ Meilisearch          │ RocksDB       │ Redis         │ RustFS    │
│ (queues)     │ (search+meta+vectors)│ (local state) │ (rate limit)  │ (archive) │
└─────────────────────────────────────────────────────────────────────────────────┘
```

### Component Details

#### 1. URL Frontier Service

```
                    ┌─────────────────────────────────────┐
                    │         URL FRONTIER SERVICE        │
                    ├─────────────────────────────────────┤
   Discovered       │                                     │      URLs to
   URLs ──────────► │  ┌─────────────────────────────┐   │ ────► Crawl
                    │  │     DEDUPLICATION LAYER     │   │
                    │  │  • Bloom filter (1B URLs)   │   │
                    │  │  • ScyllaDB lookup (exact)  │   │
                    │  └─────────────────────────────┘   │
                    │              │                     │
                    │              ▼                     │
                    │  ┌─────────────────────────────┐   │
                    │  │     PRIORITY ASSIGNMENT     │   │
                    │  │  • Domain importance        │   │
                    │  │  • Page depth               │   │
                    │  │  • Freshness score          │   │
                    │  │  • Content type hint        │   │
                    │  └─────────────────────────────┘   │
                    │              │                     │
                    │              ▼                     │
                    │  ┌─────────────────────────────┐   │
                    │  │    REDPANDA PARTITIONS      │   │
                    │  │  Partition by domain hash   │   │
                    │  │  • partition-0: a-c.com     │   │
                    │  │  • partition-1: d-f.com     │   │
                    │  │  • partition-N: ...         │   │
                    │  └─────────────────────────────┘   │
                    │              │                     │
                    │              ▼                     │
                    │  ┌─────────────────────────────┐   │
                    │  │    POLITENESS SCHEDULER     │   │
                    │  │  • Per-domain rate limits   │   │
                    │  │  • robots.txt delays        │   │
                    │  │  • Exponential backoff      │   │
                    │  └─────────────────────────────┘   │
                    └─────────────────────────────────────┘
```

#### 2. Crawler Worker

```
┌─────────────────────────────────────────────────────────────────┐
│                       CRAWLER WORKER                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐        │
│  │   URL       │    │   DNS       │    │  robots.txt │        │
│  │   Consumer  │───►│   Resolver  │───►│   Checker   │        │
│  │  (Redpanda) │    │(trust-dns)  │    │  (cached)   │        │
│  └─────────────┘    └─────────────┘    └─────────────┘        │
│                                               │                 │
│                          ┌────────────────────┴────────┐       │
│                          ▼                             ▼       │
│                 ┌─────────────────┐         ┌─────────────────┐│
│                 │   HTTP Fetcher  │         │  JS Renderer    ││
│                 │   (reqwest)     │         │ (chromiumoxide) ││
│                 │                 │         │                 ││
│                 │ • Connection    │         │ • Browser pool  ││
│                 │   pooling       │         │ • Page timeout  ││
│                 │ • Proxy rotate  │         │ • Screenshot    ││
│                 │ • Retry logic   │         │ • Wait for idle ││
│                 └─────────────────┘         └─────────────────┘│
│                          │                             │       │
│                          └────────────┬────────────────┘       │
│                                       ▼                        │
│                          ┌─────────────────────┐               │
│                          │   Response Handler  │               │
│                          │                     │               │
│                          │ • Status check      │               │
│                          │ • Content-type      │               │
│                          │ • Encoding detect   │               │
│                          │ • Redirect follow   │               │
│                          └─────────────────────┘               │
│                                       │                        │
│                                       ▼                        │
│                          ┌─────────────────────┐               │
│                          │   Link Extractor    │               │
│                          │                     │               │
│                          │ • <a href>          │               │
│                          │ • Sitemap links     │               │
│                          │ • JS-discovered     │               │
│                          └─────────────────────┘               │
│                                       │                        │
│                    ┌──────────────────┼──────────────────┐     │
│                    ▼                  ▼                  ▼     │
│            ┌─────────────┐   ┌─────────────┐   ┌─────────────┐ │
│            │  New URLs   │   │  Raw HTML   │   │   Metrics   │ │
│            │ to Frontier │   │ to Content  │   │  to Events  │ │
│            │  (Redpanda) │   │  (Redpanda) │   │  (Redpanda) │ │
│            └─────────────┘   └─────────────┘   └─────────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

#### 3. Content Processing Worker

```
┌─────────────────────────────────────────────────────────────────┐
│                    CONTENT PROCESSING WORKER                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Raw HTML                                                       │
│  from Redpanda                                                  │
│       │                                                         │
│       ▼                                                         │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                  PARSING PIPELINE                        │   │
│  ├─────────────────────────────────────────────────────────┤   │
│  │                                                         │   │
│  │  ┌──────────┐   ┌──────────┐   ┌──────────┐            │   │
│  │  │  HTML    │──►│  DOM     │──►│  Clean   │            │   │
│  │  │  Parse   │   │  Build   │   │  Content │            │   │
│  │  │(scraper) │   │ (select) │   │(readabl.)|            │   │
│  │  └──────────┘   └──────────┘   └──────────┘            │   │
│  │                                                         │   │
│  └─────────────────────────────────────────────────────────┘   │
│       │                                                         │
│       ▼                                                         │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                FEATURE EXTRACTION                        │   │
│  ├─────────────────────────────────────────────────────────┤   │
│  │                                                         │   │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐        │   │
│  │  │  Metadata  │  │  Schema    │  │  Custom    │        │   │
│  │  │  <meta>    │  │  JSON-LD   │  │  Selectors │        │   │
│  │  │  OG tags   │  │  Microdata │  │  CSS rules │        │   │
│  │  └────────────┘  └────────────┘  └────────────┘        │   │
│  │                                                         │   │
│  │  ┌────────────┐  ┌────────────┐  ┌────────────┐        │   │
│  │  │  Markdown  │  │  Block     │  │  Language  │        │   │
│  │  │  Convert   │  │  Split     │  │  Detect    │        │   │
│  │  │  (html2md) │  │  (H1-H6)   │  │  (whatlang)│        │   │
│  │  └────────────┘  └────────────┘  └────────────┘        │   │
│  │                                                         │   │
│  └─────────────────────────────────────────────────────────┘   │
│       │                                                         │
│       ▼                                                         │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                AI ENRICHMENT (Optional)                  │   │
│  ├─────────────────────────────────────────────────────────┤   │
│  │                                                         │   │
│  │  ┌────────────────┐      ┌────────────────┐            │   │
│  │  │  Extraction    │      │  Summarization │            │   │
│  │  │  (GPT/Claude)  │      │  (GPT/Claude)  │            │   │
│  │  │                │      │                │            │   │
│  │  │  Custom prompt │      │  TL;DR gen     │            │   │
│  │  │  JSON output   │      │  Key points    │            │   │
│  │  └────────────────┘      └────────────────┘            │   │
│  │                                                         │   │
│  │  ┌────────────────┐      ┌────────────────┐            │   │
│  │  │  Embedding     │      │  Classification│            │   │
│  │  │  (sentence-tx) │      │  (topic/type)  │            │   │
│  │  └────────────────┘      └────────────────┘            │   │
│  │                                                         │   │
│  └─────────────────────────────────────────────────────────┘   │
│       │                                                         │
│       ▼                                                         │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                   OUTPUT                                 │   │
│  ├─────────────────────────────────────────────────────────┤   │
│  │  • Document → Meilisearch (search)                      │   │
│  │  • Raw HTML → S3 (archive)                              │   │
│  │  • Metadata → ScyllaDB (queryable)                      │   │
│  │  • Embeddings → Vector DB (semantic search)             │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

---

## Crate Structure

```
scrapix/
├── Cargo.toml                    # Workspace root
├── Cargo.lock
│
├── crates/
│   │
│   ├── scrapix-core/            # Shared types, traits, utils
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs        # Configuration schemas
│   │       ├── document.rs      # Document types
│   │       ├── error.rs         # Error types
│   │       └── traits.rs        # Core traits
│   │
│   ├── scrapix-frontier/        # URL frontier service
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── dedup.rs         # Bloom filter deduplication
│   │       ├── priority.rs      # Priority assignment
│   │       ├── politeness.rs    # Rate limiting
│   │       ├── partition.rs     # Domain partitioning
│   │       ├── simhash.rs       # SimHash/MinHash near-duplicate detection
│   │       ├── history.rs       # URL crawl history tracking
│   │       └── recrawl.rs       # Incremental re-crawl scheduling
│   │
│   ├── scrapix-crawler/         # HTTP/JS crawler
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── fetcher.rs       # HTTP client (reqwest)
│   │       ├── renderer.rs      # JS rendering (chromiumoxide)
│   │       ├── robots.rs        # robots.txt parser
│   │       ├── dns.rs           # DNS resolver/cache
│   │       ├── proxy.rs         # Proxy rotation
│   │       └── extractor.rs     # Link extraction
│   │
│   ├── scrapix-parser/          # Content parsing
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── html.rs          # HTML parsing (scraper)
│   │       ├── readability.rs   # Content extraction
│   │       ├── markdown.rs      # HTML to Markdown
│   │       └── language.rs      # Language detection
│   │
│   ├── scrapix-extractor/       # Feature extraction
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── metadata.rs      # Meta tags, OG
│   │       ├── schema.rs        # JSON-LD, Microdata
│   │       ├── selectors.rs     # Custom CSS selectors
│   │       └── blocks.rs        # Block splitting
│   │
│   ├── scrapix-ai/              # AI enrichment
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── extraction.rs    # GPT extraction
│   │       ├── summary.rs       # Summarization
│   │       ├── embedding.rs     # Vector embeddings
│   │       └── client.rs        # OpenAI/Claude client
│   │
│   ├── scrapix-storage/         # Storage abstractions
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── meilisearch.rs   # Meilisearch client
│   │       ├── scylla.rs        # ScyllaDB client
│   │       ├── rocks.rs         # RocksDB wrapper
│   │       ├── object_storage.rs # S3/MinIO object storage
│   │       ├── clickhouse.rs    # ClickHouse analytics
│   │       └── redis.rs         # Redis/Dragonfly
│   │
│   ├── scrapix-queue/           # Message queue
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── producer.rs      # Redpanda producer
│   │       ├── consumer.rs      # Redpanda consumer
│   │       └── topics.rs        # Topic definitions
│   │
│   └── scrapix-telemetry/       # Observability
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           ├── metrics.rs       # Prometheus metrics
│           ├── tracing.rs       # Distributed tracing
│           └── logging.rs       # Structured logging
│
├── bins/
│   │
│   ├── scrapix-api/             # REST API server
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── routes/          # API endpoints
│   │       ├── auth.rs          # Authentication
│   │       └── websocket.rs     # Real-time events
│   │
│   ├── scrapix-worker-crawler/  # Crawler worker binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   ├── scrapix-worker-content/  # Content processor binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   ├── scrapix-frontier/        # Frontier service binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── main.rs
│   │
│   └── scrapix-cli/             # CLI tool
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
│
├── docker/
│   ├── Dockerfile.api
│   ├── Dockerfile.worker-crawler
│   ├── Dockerfile.worker-content
│   ├── Dockerfile.frontier
│   └── docker-compose.yml
│
├── deploy/
│   └── kubernetes/              # K8s manifests (base + overlays: local, prod)
│
└── docs/
    ├── api.md
    ├── configuration.md
    └── deployment.md
```

---

## Configuration Schema

```rust
// scrapix-core/src/config.rs

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlConfig {
    // === REQUIRED ===
    pub start_urls: Vec<String>,
    pub index_uid: String,

    // === CRAWL BEHAVIOR ===
    #[serde(default)]
    pub crawler_type: CrawlerType,  // http | browser

    #[serde(default)]
    pub max_depth: Option<u32>,

    #[serde(default)]
    pub max_pages: Option<u64>,

    // === URL CONTROL ===
    #[serde(default)]
    pub url_patterns: UrlPatterns,

    #[serde(default)]
    pub sitemap: SitemapConfig,

    // === PERFORMANCE ===
    #[serde(default)]
    pub concurrency: ConcurrencyConfig,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    // === PROXY ===
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,

    // === FEATURES ===
    #[serde(default)]
    pub features: FeaturesConfig,

    // === OUTPUT ===
    #[serde(default)]
    pub meilisearch: MeilisearchConfig,

    // === WEBHOOKS ===
    #[serde(default)]
    pub webhooks: Vec<WebhookConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlerType {
    #[default]
    Http,      // reqwest only (fast)
    Browser,   // chromiumoxide (JS)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UrlPatterns {
    pub include: Vec<String>,      // Glob patterns to include
    pub exclude: Vec<String>,      // Glob patterns to exclude
    pub index_only: Vec<String>,   // Only index these (crawl all)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SitemapConfig {
    pub enabled: bool,
    pub urls: Vec<String>,         // Custom sitemap URLs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    #[serde(default = "default_concurrency")]
    pub max_concurrent_requests: u32,

    #[serde(default = "default_browser_pool")]
    pub browser_pool_size: u32,
}

fn default_concurrency() -> u32 { 50 }
fn default_browser_pool() -> u32 { 5 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default)]
    pub requests_per_second: Option<f64>,

    #[serde(default)]
    pub requests_per_minute: Option<u32>,

    #[serde(default)]
    pub per_domain_delay_ms: Option<u64>,

    #[serde(default = "default_true")]
    pub respect_robots_txt: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub urls: Vec<String>,

    #[serde(default)]
    pub rotation: ProxyRotation,

    #[serde(default)]
    pub tiered: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyRotation {
    #[default]
    RoundRobin,
    Random,
    LeastUsed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FeaturesConfig {
    pub metadata: Option<FeatureToggle>,
    pub markdown: Option<FeatureToggle>,
    pub block_split: Option<FeatureToggle>,
    pub schema: Option<SchemaFeatureConfig>,
    pub custom_selectors: Option<CustomSelectorsConfig>,
    pub ai_extraction: Option<AiExtractionConfig>,
    pub ai_summary: Option<FeatureToggle>,
    pub embeddings: Option<EmbeddingsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureToggle {
    pub enabled: bool,
    #[serde(default)]
    pub include_pages: Vec<String>,
    #[serde(default)]
    pub exclude_pages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaFeatureConfig {
    pub enabled: bool,
    #[serde(default)]
    pub only_types: Vec<String>,    // ["Product", "Article"]
    #[serde(default)]
    pub convert_dates: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomSelectorsConfig {
    pub enabled: bool,
    pub selectors: std::collections::HashMap<String, SelectorDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelectorDef {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiExtractionConfig {
    pub enabled: bool,
    pub prompt: String,
    #[serde(default)]
    pub model: String,           // gpt-4, claude-3, etc.
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingsConfig {
    pub enabled: bool,
    pub model: String,           // text-embedding-3-small
    pub dimensions: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeilisearchConfig {
    pub url: String,
    pub api_key: String,

    #[serde(default)]
    pub primary_key: Option<String>,

    #[serde(default)]
    pub settings: Option<MeilisearchSettings>,

    #[serde(default)]
    pub batch_size: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeilisearchSettings {
    pub searchable_attributes: Option<Vec<String>>,
    pub filterable_attributes: Option<Vec<String>>,
    pub sortable_attributes: Option<Vec<String>>,
    pub ranking_rules: Option<Vec<String>>,
    pub stop_words: Option<Vec<String>>,
    pub synonyms: Option<std::collections::HashMap<String, Vec<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    pub url: String,
    pub events: Vec<WebhookEvent>,

    #[serde(default)]
    pub auth: Option<WebhookAuth>,

    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEvent {
    CrawlStarted,
    CrawlCompleted,
    CrawlFailed,
    ProgressUpdate,
    PageCrawled,
    PageIndexed,
    PageError,
    BatchSent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookAuth {
    Bearer { token: String },
    Hmac { secret: String, algorithm: String },
    Headers { headers: std::collections::HashMap<String, String> },
}
```

---

## Deployment (Kubernetes Only)

### Local (Docker Desktop Kubernetes)

Run the full stack on your laptop using Docker Desktop with Kubernetes enabled (or Minikube/kind). A `deploy/kubernetes` layout with Kustomize overlays is assumed:

```
deploy/
  kubernetes/
    base/
      namespace.yaml
      api-deployment.yaml
      crawler-worker-deployment.yaml
      content-worker-deployment.yaml
      frontier-statefulset.yaml
      redpanda-statefulset.yaml
      meilisearch-statefulset.yaml
      dragonfly-deployment.yaml
      services/*.yaml
      config/*.yaml
    overlays/
      local/
        kustomization.yaml
        patches/*.yaml         # dev images, NodePort/LoadBalancer, small resources
      prod/
        kustomization.yaml
        patches/*.yaml         # HPA, PDBs, TLS, resource requests/limits
```

Quickstart (local):

1) Prereqs: Docker Desktop with Kubernetes, `kubectl`, `kustomize` (or `kubectl kustomize`), optional `helm`
2) Create namespace and apply local overlay:
```
kubectl create namespace scrapix --dry-run=client -o yaml | kubectl apply -f -
kubectl apply -k deploy/kubernetes/overlays/local
```
3) Access services:
- API: `kubectl port-forward -n scrapix deploy/scrapix-api 8080:8080`
- Meilisearch: `kubectl port-forward -n scrapix statefulset/meilisearch 7700:7700`
- Redpanda UI (if enabled): port-forward corresponding service

Notes:
- Local overlay uses smaller resource requests/limits and single-node StatefulSets.
- Persistent volumes rely on the Docker Desktop storage class (or hostPath on kind).
- Swap out Redpanda/Meilisearch for their official Helm charts if preferred.

### Production (Kubernetes)

Cluster layout:
```
Namespaces:
├── scrapix           # app workloads (api, workers, frontier)
├── data              # stateful data planes (redpanda, meilisearch, dragonfly)
└── monitoring        # Prometheus, Grafana, Jaeger
```

Guidelines:
- Use HPAs for `scrapix-api`, `scrapix-worker-crawler`, `scrapix-worker-content`.
- Use StatefulSets with persistent volumes for Redpanda and Meilisearch.
- Configure PodDisruptionBudgets, PodAntiAffinity, and appropriate resource requests/limits.
- Ingress with TLS (e.g., cert-manager + NGINX/Traefik).
- Centralized metrics/tracing via `scrapix-telemetry` and standard exporters.

---

## Scaling Targets

### Phase 1: MVP (1M pages/day)
- 5 crawler workers
- 3 content workers
- 1 frontier instance
- Redpanda: 3 partitions
- Meilisearch: Single node

### Phase 2: Growth (10M pages/day)
- 20 crawler workers
- 10 content workers
- 3 frontier instances
- Redpanda: 10 partitions
- Meilisearch: 3-node cluster

### Phase 3: Scale (100M pages/day)
- 100 crawler workers
- 50 content workers
- 10 frontier instances (sharded)
- Redpanda: 50 partitions
- Meilisearch: 10-node cluster
- ScyllaDB: 6-node cluster

### Phase 4: Internet-Scale (1B+ pages/day)
- 1000+ crawler workers (multi-region)
- 500+ content workers
- Sharded frontier across regions
- Redpanda: Multi-cluster federation
- Custom storage layer

---

## Next Steps

1. **Initialize Rust workspace** with basic crate structure
2. **Implement scrapix-core** with config and types
3. **Build scrapix-crawler** with reqwest + basic fetching
4. **Add scrapix-parser** with HTML parsing
5. **Create scrapix-api** with basic endpoints
6. **Integrate Meilisearch** for indexing
7. **Add Redpanda** for queue
8. **Deploy MVP** to local Kubernetes (Docker Desktop)

---

## References

### Message Queue
- [Redpanda vs Kafka Performance](https://www.redpanda.com/blog/redpanda-vs-kafka-performance-benchmark)
- [Kafka vs Pulsar vs NATS Comparison](https://risingwave.com/blog/kafka-pulsar-and-nats-a-comprehensive-comparison-of-messaging-systems/)
- [BuildShift: Which Message Broker in 2025](https://medium.com/@BuildShift/kafka-is-old-redpanda-is-fast-pulsar-is-weird-nats-is-tiny-which-message-broker-should-you-32ce61d8aa9f)

### Search Engines
- [Meilisearch vs Typesense](https://www.meilisearch.com/blog/meilisearch-vs-typesense)
- [Typesense Comparison](https://typesense.org/typesense-vs-algolia-vs-elasticsearch-vs-meilisearch/)

### Crawler Architecture
- [Web Crawler System Design](https://grokkingthesystemdesign.com/guides/web-crawler-system-design/)
- [Design a Web Crawler](https://www.designgurus.io/blog/design-web-crawler)
- [Stanford IR Book: Crawler Architecture](https://nlp.stanford.edu/IR-book/html/htmledition/crawler-architecture-1.html)

### Rust Libraries
- [Spider-rs](https://github.com/spider-rs/spider)
- [Rust Web Scraping Guide](https://www.zenrows.com/blog/rust-web-scraping)
- [reqwest](https://docs.rs/reqwest)
- [scraper](https://docs.rs/scraper)
- [chromiumoxide](https://docs.rs/chromiumoxide)

### Databases
- [RocksDB vs ScyllaDB vs Redis](https://db-engines.com/en/system/Redis%3BRocksDB%3BScyllaDB)
- [Bloom Filters in Distributed Crawlers](https://ieeexplore.ieee.org/document/8560281/)
