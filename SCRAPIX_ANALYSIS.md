# Scrapix Architecture Analysis
## Complete Technical Deep-Dive for Redesign

**Purpose**: This document provides a comprehensive analysis of Scrapix to serve as the foundation for rebuilding a next-generation web crawling and indexing platform capable of scaling to millions (or billions) of pages.

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Current Architecture Overview](#2-current-architecture-overview)
3. [Crawler System](#3-crawler-system)
4. [Scraper & Feature Pipeline](#4-scraper--feature-pipeline)
5. [Document Flow & Data Model](#5-document-flow--data-model)
6. [Server & Job Queue Architecture](#6-server--job-queue-architecture)
7. [Proxy System](#7-proxy-system)
8. [Configuration Options](#8-configuration-options)
9. [Deployment & Infrastructure](#9-deployment--infrastructure)
10. [Current Limitations & Bottlenecks](#10-current-limitations--bottlenecks)
11. [Design Patterns Used](#11-design-patterns-used)
12. [Recommendations for Next Generation](#12-recommendations-for-next-generation)

---

## 1. Executive Summary

### What Scrapix Does

Scrapix is a **TypeScript monorepo** providing:
- **Web Crawling**: Discovers and fetches web pages (Cheerio/Puppeteer/Playwright)
- **Content Extraction**: Extracts structured data via configurable feature pipeline
- **AI Enhancement**: GPT-powered extraction and summarization
- **Search Indexing**: Sends documents to Meilisearch for full-text search

### Core Flow
```
URL Discovery → Page Fetch → Content Extraction → Feature Pipeline → Batch Send → Meilisearch Index
```

### Tech Stack
| Component | Technology |
|-----------|------------|
| Language | TypeScript (Node.js) |
| Crawling | Crawlee (Cheerio, Puppeteer, Playwright) |
| Job Queue | Bull (Redis-backed) |
| Search | Meilisearch |
| API | Express.js |
| Deployment | Fly.io + Upstash Redis |
| AI | OpenAI GPT API |

### Project Structure
```
apps/
├── core/      # Core crawling library (@scrapix/core npm package)
├── server/    # REST API with job queue
├── cli/       # Command-line interface
└── proxy/     # Proxy server for distributed crawling
```

---

## 2. Current Architecture Overview

### High-Level Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CLIENT LAYER                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│  CLI (yarn scrape)          │  REST API (/crawl)      │  SSE (/job/:id/events)
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                                       ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           SERVER LAYER (Express.js)                          │
├─────────────────────────────────────────────────────────────────────────────┤
│  Rate Limiting │ Validation │ Job Queue (Bull) │ Event Bus │ Telemetry      │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
                    ┌──────────────────┼──────────────────┐
                    ▼                  ▼                  ▼
┌───────────────────────┐ ┌───────────────────┐ ┌───────────────────────────┐
│     REDIS (Upstash)   │ │  CHILD PROCESS    │ │     SUPABASE (optional)   │
│  - Job persistence    │ │  - crawler_process│ │  - Config storage         │
│  - Event queuing      │ │  - IPC messages   │ │  - Run history            │
│  - Metrics            │ │  - Isolated exec  │ │  - Event persistence      │
└───────────────────────┘ └───────────────────┘ └───────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            CORE LAYER (Crawlee)                              │
├─────────────────────────────────────────────────────────────────────────────┤
│  CrawlerFactory    │  BaseCrawler (abstract)  │  RequestQueue │  ProxyConfig │
├────────────────────┼─────────────────────────┼───────────────┼──────────────┤
│  CheerioCrawler    │  PuppeteerCrawler       │  PlaywrightCrawler (beta)   │
└─────────────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                         SCRAPER LAYER (Feature Pipeline)                     │
├─────────────────────────────────────────────────────────────────────────────┤
│  full_page → metadata → custom_selectors → markdown → schema → ai_* → block │
└─────────────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            SENDER LAYER                                      │
├─────────────────────────────────────────────────────────────────────────────┤
│  Document Queue │ Batch Send │ Temp Index │ Atomic Swap │ Webhooks          │
└─────────────────────────────────────────────────────────────────────────────┘
                                   │
                                   ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                            MEILISEARCH                                       │
├─────────────────────────────────────────────────────────────────────────────┤
│  Primary Index: {uid}        │    Temp Index: {uid}_crawler_tmp             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Crawler System

### 3.1 Factory Pattern

```typescript
// CrawlerFactory creates instances based on type
Crawler.create(type, config, container)
  ├── type: 'cheerio' | 'puppeteer' | 'playwright'
  ├── config: ConfigSchema (validated with Zod)
  └── container: DI container (Meilisearch, Sender, Logger)
```

### 3.2 Crawler Types Comparison

| Crawler | Engine | JavaScript | Speed | Memory | Best For |
|---------|--------|------------|-------|--------|----------|
| **Cheerio** | HTTP + Cheerio | No | Fastest | Low | Static sites, docs, blogs |
| **Puppeteer** | Chrome/Chromium | Yes | Slow | High | SPAs, dynamic content |
| **Playwright** | Multi-browser | Yes | Slow | High | Cross-browser, complex interactions |

### 3.3 BaseCrawler (Abstract Class)

**Key Properties:**
```typescript
class BaseCrawler {
  sender: Sender              // Document batching & sending
  config: Config              // Validated configuration
  urls: string[]              // Discovered URLs
  scraper: Scraper            // Feature pipeline processor
  nb_page_crawled: number     // Crawl counter
  nb_page_indexed: number     // Index counter
}
```

**Key Methods:**
```typescript
// Abstract (implemented by each crawler type)
abstract createRouter(): Router
abstract getCrawlerOptions(): CrawlerOptions
abstract createCrawlerInstance(options): CrawlerInstance

// Protected (shared logic)
__generate_globs(urls)        // Convert URLs to glob patterns
__match_globs(url)            // Match URL against patterns
__is_file_url(url)            // Filter 70+ file extensions
__is404Page($, context)       // Detect 404 pages
handlePage($, context)        // Core page processing
```

### 3.4 URL Filtering Logic

```
1. URL Discovery (from page links or sitemap)
        │
        ▼
2. File Extension Check (__is_file_url)
   - Filters: .pdf, .jpg, .zip, .mp4, etc. (70+ extensions)
        │
        ▼
3. Exclusion Check (urls_to_exclude)
   - Glob pattern matching via minimatch
        │
        ▼
4. Scope Check (__match_globs)
   - Must match start_urls scope patterns
        │
        ▼
5. 404 Detection (__is404Page)
   - CSS selectors or text content matching
        │
        ▼
6. Index Decision (urls_to_index / urls_to_not_index)
   - Crawl but don't index, or skip entirely
```

### 3.5 Request Queue Management

Crawlee's `RequestQueue` handles:
- **Deduplication**: Same URL not crawled twice
- **Priority**: Can prioritize certain URLs
- **Persistence**: File-based by default (disabled in Docker)
- **Retry Logic**: Failed requests re-queued with backoff

---

## 4. Scraper & Feature Pipeline

### 4.1 Pipeline Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         SCRAPER PIPELINE                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│   HTML/DOM ($)                                                         │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  1. full_page (always runs)                                     │  │
│   │     - Extract <main> or <body>                                  │  │
│   │     - Build block structure (H1-H6 hierarchy)                   │  │
│   │     - Generate UUID, extract title, domain, url_tags            │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  2. metadata (if activated)                                     │  │
│   │     - <meta name>, <meta property> (OpenGraph)                  │  │
│   │     - Twitter card tags                                         │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  3. custom_selectors (if activated)                             │  │
│   │     - CSS selector extraction                                   │  │
│   │     - Returns { field: value } from selectors config            │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  4. markdown (if activated)                                     │  │
│   │     - HTML to Markdown conversion                               │  │
│   │     - Preserves structure for LLM processing                    │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  5. schema (if activated)                                       │  │
│   │     - JSON-LD extraction                                        │  │
│   │     - Microdata extraction                                      │  │
│   │     - RDFa extraction                                           │  │
│   │     - Optional type filtering (only_type: "Product")            │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  6. ai_extraction (if activated, requires OPENAI_API_KEY)       │  │
│   │     - Send content to GPT with custom prompt                    │  │
│   │     - Parse JSON response                                       │  │
│   │     - Max content: 8000 chars                                   │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  7. ai_summary (if activated, requires OPENAI_API_KEY)          │  │
│   │     - Generate summary via GPT                                  │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   ┌─────────────────────────────────────────────────────────────────┐  │
│   │  8. block_split (if activated)                                  │  │
│   │     - Split FullPageDocument into BlockDocuments                │  │
│   │     - One block per semantic section                            │  │
│   │     - Maintains parent_document_id relationship                 │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│       │                                                                 │
│       ▼                                                                 │
│   Document(s) ready for indexing                                       │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 4.2 Feature Activation Pattern

Each feature supports:
```typescript
{
  activated: boolean,           // Enable/disable
  include_pages: string[],      // Only these URLs (glob)
  exclude_pages: string[]       // Skip these URLs (glob)
}
```

### 4.3 Processing Order (Important)

The order is fixed and intentional:
1. **full_page** - Always first (creates base document)
2. **metadata** - Early (adds meta fields)
3. **custom_selectors** - Early (adds custom fields)
4. **markdown** - Middle (transforms content)
5. **schema** - Middle (adds structured data)
6. **ai_extraction** - Late (needs full content)
7. **ai_summary** - Late (needs full content)
8. **block_split** - Always last (splits document)

---

## 5. Document Flow & Data Model

### 5.1 Document Types

**FullPageDocument** (initial):
```typescript
interface FullPageDocument {
  uid: string                    // UUID v4
  url: string                    // Source URL
  domain: string                 // Hostname
  title?: string                 // <title> tag
  urls_tags?: string[]           // URL path segments
  blocks: Array<{
    h1?: string                  // Heading hierarchy
    h2?: string
    h3?: string
    h4?: string
    h5?: string
    h6?: string
    p?: string                   // Paragraph content
    anchor?: string              // Element ID
  }>
  // Added by features:
  metadata?: Record<string, string>
  custom?: Record<string, string | string[]>
  markdown?: string
  schema?: Record<string, any>
  ai_extraction?: Record<string, any>
  ai_summary?: string
}
```

**BlockDocument** (after block_split):
```typescript
interface BlockDocument {
  uid: string                    // New UUID per block
  parent_document_id: string     // Original document ID
  page_block: number             // Block index (0, 1, 2...)
  url: string                    // Original URL
  domain: string
  title?: string
  h1?: string                    // Heading at this level
  h2?: string
  h3?: string
  h4?: string
  h5?: string
  p?: string | string[]          // Block content
  anchor?: string                // Jump link
  // All feature data inherited
}
```

### 5.2 Document Batching & Sending

```
Document Created
      │
      ▼
Sender.add(document)
      │
      ▼
Add to Queue (in-memory)
      │
      ▼
Queue.length >= batch_size?
      │
      ├── No: Wait for more
      │
      └── Yes: __batchSend()
              │
              ▼
         Async HTTP POST to Meilisearch
              │
              ▼
         Track pending tasks
```

**Batch Configuration:**
- Default batch size: 1000 documents
- Retry with exponential backoff (1s, 2s, 4s... up to 32s)
- Async non-blocking sends during crawl
- Sync final flush on completion

### 5.3 Index Swapping Strategy

```
Start Crawl
    │
    ├── Index exists?
    │       │
    │       ├── Yes: Create temp index {uid}_crawler_tmp
    │       │        Use temp for all writes
    │       │
    │       └── No: Create new index {uid}
    │
    ▼
Crawl & Index Documents
    │
    ▼
Finish
    │
    ├── Temp index has documents?
    │       │
    │       ├── Yes: meilisearch.swapIndexes([{uid}, {uid}_crawler_tmp])
    │       │        Delete temp index
    │       │
    │       └── No: Keep original index
    │
    ▼
Done (atomic swap = no search downtime)
```

---

## 6. Server & Job Queue Architecture

### 6.1 API Endpoints

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/health` | GET | Server health check |
| `/crawl` | POST | Async crawl (returns jobId) |
| `/crawl/async` | POST | Alias for /crawl |
| `/crawl/sync` | POST | Blocking crawl (waits for completion) |
| `/job/:id/status` | GET | Job state, progress, timestamps |
| `/job/:id/events` | GET | SSE stream for real-time events |
| `/events/stats` | GET | Event system statistics |

### 6.2 Job Queue (Bull + Redis)

```
POST /crawl
    │
    ▼
Validate Config (Zod)
    │
    ▼
Add to Bull Queue
    │
    ▼
Return { jobId, indexUid, statusUrl, eventsUrl }
    │
    ▼
Queue picks job when capacity available
    │
    ▼
Fork child process (crawler_process.js)
    │
    ▼
IPC Communication:
    ├── crawler.event (page events)
    ├── batch.processed (batch updates)
    └── progress (every 5 seconds)
    │
    ▼
Forward to SSE clients
    │
    ▼
Job Complete/Failed
```

### 6.3 Event System

**EventBus** (Singleton):
- Centralized event management
- Event batching (50 events or 5s timeout)
- Metrics tracking
- SSE broadcasting

**Events Emitted:**
```typescript
CRAWLER_STARTED      // Begin crawl
CRAWLER_COMPLETED    // Finish with stats
PAGE_CRAWLED         // Each page fetched
PAGE_INDEXED         // Document sent
PAGE_ERROR           // Processing failure
BATCH_SENT           // Meilisearch batch
PROGRESS_UPDATE      // Aggregated metrics
```

### 6.4 SSE (Server-Sent Events)

```
Client connects to /job/:id/events
    │
    ▼
Register in sseClients Map
    │
    ▼
Receive events in real-time:
    - connected
    - crawler.event
    - job.status
    - job.completed / job.failed
    - batch.processed
    - ping (keep-alive every 30s)
    │
    ▼
Auto-cleanup on disconnect or 5min stale
```

---

## 7. Proxy System

### 7.1 Architecture

The proxy (`apps/proxy/`) runs as a separate Fly.io app for distributed crawling.

**Dual Server Model:**
1. **Proxy Server** (Port 8080) - HTTP/HTTPS CONNECT tunneling
2. **Management Interface** (Port 3000) - Health, stats, logs

### 7.2 Proxy Configuration

**Simple Rotation:**
```json
{
  "proxy_configuration": {
    "proxyUrls": ["http://proxy1:8080", "http://proxy2:8080"]
  }
}
```

**Tiered Fallback:**
```json
{
  "proxy_configuration": {
    "tieredProxyUrls": [
      [null],                           // Tier 0: Direct
      ["http://backup:8080"],           // Tier 1: Backup
      ["http://premium1:8080", "http://premium2:8080"]  // Tier 2: Premium
    ]
  }
}
```

### 7.3 Features

- Request tracking with UUID
- Daily stats reset
- Region header injection (`X-Scrapix-Region`)
- Authentication (Bearer, Basic)
- 30-second timeout
- In-memory log buffer (1000 entries)

---

## 8. Configuration Options

### 8.1 Required Fields

```json
{
  "start_urls": ["https://example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "my_index"
}
```

### 8.2 Complete Configuration Reference

```json
{
  // === REQUIRED ===
  "start_urls": ["https://example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "my_index",

  // === CRAWLER TYPE ===
  "crawler_type": "cheerio",  // "cheerio" | "puppeteer" | "playwright"

  // === URL CONTROL ===
  "urls_to_exclude": ["https://example.com/admin/**"],
  "urls_to_index": ["https://example.com/docs/**"],
  "urls_to_not_index": ["https://example.com/login"],
  "use_sitemap": true,
  "sitemap_urls": ["https://example.com/sitemap.xml"],

  // === PERFORMANCE ===
  "max_concurrency": 10,
  "max_requests_per_minute": 60,
  "max_request_retries": 3,
  "batch_size": 1000,

  // === PROXY ===
  "proxy_configuration": {
    "proxyUrls": ["http://proxy:8080"],
    "tieredProxyUrls": [[null], ["http://proxy:8080"]]
  },

  // === REQUEST ===
  "additional_request_headers": { "Authorization": "Bearer token" },
  "user_agents": ["Mozilla/5.0..."],
  "launch_options": { "headless": true },

  // === MEILISEARCH ===
  "primary_key": "url",
  "meilisearch_settings": {
    "searchableAttributes": ["title", "content"],
    "filterableAttributes": ["domain"],
    "sortableAttributes": ["date"]
  },
  "keep_settings": false,

  // === FEATURES ===
  "features": {
    "metadata": { "activated": true },
    "markdown": { "activated": true },
    "block_split": { "activated": true },
    "custom_selectors": {
      "activated": true,
      "selectors": { "price": ".price" }
    },
    "schema": {
      "activated": true,
      "only_type": "Product",
      "convert_dates": true
    },
    "ai_extraction": {
      "activated": true,
      "prompt": "Extract product details"
    },
    "ai_summary": { "activated": true }
  },

  // === WEBHOOKS ===
  "webhook_url": "https://example.com/webhook",
  "webhook_payload": { "source": "scrapix" },
  "webhooks": {
    "main": {
      "url": "https://example.com/webhook",
      "auth": { "bearer": "token" },
      "events": ["run.completed"],
      "enabled": true
    }
  },

  // === ERROR DETECTION ===
  "not_found_selectors": [".error-404", "h1:contains('Not Found')"]
}
```

---

## 9. Deployment & Infrastructure

### 9.1 Current Stack

| Component | Service | Purpose |
|-----------|---------|---------|
| Server | Fly.io (`scrapix.fly.dev`) | API + job processing |
| Proxy | Fly.io (`scrapix-proxy.fly.dev`) | Distributed proxies |
| Queue | Upstash Redis | Job persistence |
| Search | Meilisearch (self-hosted or cloud) | Document storage |
| Storage | Supabase (optional) | Config + run history |

### 9.2 Fly.io Configuration

**Server** (`apps/server/fly.toml`):
```toml
app = "scrapix"
primary_region = "iad"

[http_service]
  internal_port = 8080
  min_machines_running = 0  # Auto-suspend
  [concurrency]
    hard_limit = 200
    soft_limit = 150

[vm]
  memory = 1024
```

**Proxy** (`apps/proxy/fly.toml`):
```toml
app = "scrapix-proxy"
primary_region = "iad"

[http_service]
  internal_port = 3000
  min_machines_running = 1  # Always on

[[services]]
  protocol = "tcp"
  internal_port = 8080
  [concurrency]
    hard_limit = 500
```

### 9.3 Docker Configuration

**Server Dockerfile:**
- Base: `ghcr.io/puppeteer/puppeteer:23.9.0` (Chrome pre-installed)
- Non-root user execution
- Puppeteer paths configured

**Proxy Dockerfile:**
- Base: `node:18-alpine`
- Lightweight (no browser)

### 9.4 Environment Variables

```bash
# Required
REDIS_URL=rediss://...@upstash.io:6379
MEILISEARCH_HOST=http://...
MEILISEARCH_API_KEY=...

# Optional
OPENAI_API_KEY=sk-...
SUPABASE_URL=https://...
SUPABASE_ANON_KEY=...
WEBHOOK_SECRET=...

# Proxy
PROXY_AUTH_ENABLED=true
PROXY_AUTH_TOKEN=...
```

---

## 10. Current Limitations & Bottlenecks

### 10.1 Scalability Issues

| Limitation | Impact | Root Cause |
|------------|--------|------------|
| **Single-threaded Node.js** | CPU-bound operations block | JavaScript event loop |
| **In-memory event buffer** | Memory growth with events | No persistent event store |
| **Cheerio concurrency=1** | Sequential crawling | Crawlee storage constraints |
| **Single Redis queue** | Queue bottleneck at scale | Centralized queue |
| **File-based RequestQueue** | I/O bottleneck | Crawlee default storage |
| **Puppeteer memory** | ~200MB per browser | Chrome overhead |

### 10.2 Architecture Limitations

| Area | Current | Limitation |
|------|---------|------------|
| **Horizontal Scaling** | 1 job per machine | No distributed crawling |
| **State Management** | In-memory + Redis | No persistent URL frontier |
| **Deduplication** | Per-job only | No global seen URLs |
| **Politeness** | Per-crawler | No centralized rate limiting |
| **Fault Tolerance** | Job retry only | No checkpoint/resume |

### 10.3 Missing Features for Internet-Scale

| Feature | Status | Needed For |
|---------|--------|------------|
| **Distributed Frontier** | Missing | Global URL queue |
| **Consistent Hashing** | Missing | URL → Worker routing |
| **Bloom Filter** | Missing | Efficient dedup |
| **Robots.txt Cache** | Basic | Politeness at scale |
| **DNS Cache** | Missing | Performance |
| **Content Fingerprinting** | Missing | Near-duplicate detection |
| **Link Graph** | Missing | PageRank-like scoring |
| **Incremental Crawling** | Missing | Re-crawl changed pages |

---

## 11. Design Patterns Used

### 11.1 Patterns Summary

| Pattern | Location | Purpose |
|---------|----------|---------|
| **Factory** | CrawlerFactory | Create crawler instances by type |
| **Strategy** | Cheerio/Puppeteer/Playwright | Interchangeable crawling algorithms |
| **Template Method** | BaseCrawler | Shared crawling logic with hooks |
| **Pipeline** | Scraper features | Sequential document transformation |
| **Singleton** | EventBus | Centralized event management |
| **Dependency Injection** | Container | Loose coupling of services |
| **Observer** | EventEmitter | Event-driven architecture |
| **Builder** | Document construction | Step-by-step document building |

### 11.2 Good Design Decisions

1. **Separation of Concerns**: Core library independent of server
2. **Configurable Features**: Enable/disable without code changes
3. **Atomic Index Swaps**: Zero-downtime updates
4. **Event-Driven**: Real-time progress tracking
5. **Zod Validation**: Type-safe configuration

### 11.3 Technical Debt

1. **Tight Crawlee Coupling**: Hard to swap crawling engine
2. **Limited Testing**: No comprehensive test suite
3. **Hardcoded Constants**: Magic numbers in code
4. **Mixed Async Patterns**: Callbacks + Promises + async/await

---

## 12. Recommendations for Next Generation

### 12.1 Target Goals

Based on user requirements:
1. **Scale to millions/billions of pages** (Google-like)
2. **Support specific website/documentation indexing** (current use case)
3. **Real-time information retrieval** (like Exa.ai)
4. **Global internet index** (ambitious long-term)

### 12.2 Language Considerations

| Language | Pros | Cons | Best For |
|----------|------|------|----------|
| **Rust** | Performance, safety, async | Learning curve, ecosystem | Core crawler engine |
| **Go** | Simplicity, concurrency, fast compile | GC pauses, less expressive | API services, workers |
| **Python** | AI/ML ecosystem, rapid dev | GIL, slower | AI extraction, prototyping |
| **TypeScript** | Current codebase, ecosystem | Performance ceiling | Orchestration, API |

**Recommendation**: **Rust for core crawler** + **Go for services** + **TypeScript for API/orchestration**

### 12.3 Architecture for Scale

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ORCHESTRATION LAYER                               │
├─────────────────────────────────────────────────────────────────────────────┤
│  API Gateway │ Job Scheduler │ Rate Limiter │ Config Management             │
└─────────────────────────────────────────────────────────────────────────────┘
                                       │
           ┌───────────────────────────┼───────────────────────────┐
           ▼                           ▼                           ▼
┌─────────────────────┐   ┌─────────────────────┐   ┌─────────────────────┐
│   URL FRONTIER      │   │   CRAWLER WORKERS   │   │   CONTENT WORKERS   │
│  (Distributed)      │   │   (Rust/Go)         │   │   (Rust/Go)         │
├─────────────────────┤   ├─────────────────────┤   ├─────────────────────┤
│  - Priority Queue   │   │  - HTTP Fetching    │   │  - HTML Parsing     │
│  - Politeness       │   │  - JS Rendering     │   │  - Feature Extract  │
│  - Deduplication    │   │  - Proxy Rotation   │   │  - AI Enrichment    │
│  - Domain Sharding  │   │  - DNS Caching      │   │  - Document Build   │
└─────────────────────┘   └─────────────────────┘   └─────────────────────┘
           │                           │                           │
           ▼                           ▼                           ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                          DATA LAYER                                          │
├─────────────────────────────────────────────────────────────────────────────┤
│  Kafka/Pulsar (URLs) │ Redis Cluster (State) │ S3 (Content) │ Meilisearch  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### 12.4 Key Components to Build

1. **Distributed URL Frontier**
   - Apache Kafka or Pulsar for URL queue
   - Consistent hashing for domain → worker mapping
   - Bloom filter for deduplication
   - Politeness scheduler (per-domain rate limiting)

2. **High-Performance Crawler**
   - Rust with `reqwest` + `tokio` for async HTTP
   - Connection pooling
   - DNS caching
   - HTTP/2 support

3. **Headless Browser Pool**
   - Playwright/Puppeteer pool manager
   - Auto-scaling based on JS-heavy URLs
   - Resource isolation

4. **Content Processing Pipeline**
   - Stream processing (Kafka Streams / Flink)
   - Feature extractors as microservices
   - AI enrichment queue

5. **Storage Strategy**
   - **Hot**: Meilisearch (search)
   - **Warm**: ClickHouse (analytics)
   - **Cold**: S3 (raw content archive)

### 12.5 Hosting Options

| Option | Best For | Cost | Complexity |
|--------|----------|------|------------|
| **Fly.io** | Current scale, edge deployment | $$ | Low |
| **AWS ECS/EKS** | Large scale, full control | $$$ | High |
| **GCP Cloud Run** | Serverless, auto-scale | $$ | Medium |
| **Self-hosted K8s** | Maximum control | $-$$$$ | Very High |
| **Modal.com** | GPU/AI workloads | $$ | Low |
| **Railway** | Simple deployment | $ | Very Low |

**Recommendation for scale**: **AWS/GCP Kubernetes** with:
- Managed Kafka (Confluent/AWS MSK)
- Redis Cluster (ElastiCache)
- S3 for content storage
- Meilisearch Cloud or self-hosted cluster

### 12.6 Next Steps

1. **Phase 1: Foundation**
   - Design new architecture document
   - Choose tech stack
   - Set up infrastructure
   - Build core crawler in Rust/Go

2. **Phase 2: MVP**
   - URL frontier with Kafka
   - Basic crawler workers
   - Meilisearch integration
   - API for job submission

3. **Phase 3: Features**
   - AI extraction pipeline
   - Headless browser support
   - Real-time API (like Exa)
   - Dashboard & monitoring

4. **Phase 4: Scale**
   - Multi-region deployment
   - Global deduplication
   - Incremental crawling
   - Link graph analysis

---

## Appendix A: File Extension Filter List

The crawler filters these file types (70+ extensions):
```
Data: json, csv, yaml, yml, xml, sql, db, sqlite, mdb
Documents: md, txt, pdf, doc, docx, xls, xlsx, ppt, pptx, odt, rtf
Archives: zip, rar, tar, gz, tgz, 7z, bz2, xz
Executables: exe, bin, apk, dmg, iso, msi, deb, rpm
Media: mp3, mp4, avi, mov, mkv, wmv, flv, webm, wav, flac, ogg, aac, m4a
Web Assets: css, js
Images: jpg, jpeg, png, gif, svg, bmp, ico, webp, tiff, psd, ai, eps
```

## Appendix B: Event Types Reference

```typescript
// Crawler Events
CRAWLER_STARTED     // { urls, config }
CRAWLER_COMPLETED   // { duration, crawled, indexed }
PAGE_CRAWLED        // { url, success, duration }
PAGE_INDEXED        // { url, features, documentCount }
PAGE_ERROR          // { url, error }

// Batch Events
BATCH_SENT          // { batchSize, batchNumber }

// Progress Events
PROGRESS_UPDATE     // { crawled, indexed, sent, errors, rate }

// Webhook Events
run.started
run.completed
run.failed
progress.update
page.crawled
page.indexed
page.error
batch.sent
```

---

**Document Version**: 1.0
**Created**: 2025-01-04
**Purpose**: Foundation for next-generation web crawler redesign
