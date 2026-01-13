# Scrapix Gap Analysis: Firecrawl & Exa.ai Competition

This document analyzes what's missing in Scrapix to compete with Firecrawl.dev and Exa.ai.

---

## Current State Summary

Scrapix has a **solid infrastructure foundation**:

| Feature | Status |
|---------|--------|
| Distributed crawling architecture | ✅ Solid |
| URL deduplication (Bloom filters) | ✅ Solid |
| Near-duplicate detection (SimHash/MinHash) | ✅ Solid |
| Priority scheduling & politeness | ✅ Solid |
| Robots.txt compliance | ✅ Solid |
| JavaScript rendering (CDP) | ✅ Works |
| Metadata extraction (OG, Twitter, Dublin Core) | ✅ Solid |
| Schema.org/JSON-LD extraction | ✅ Solid |
| HTML → Markdown conversion | ✅ Solid |
| Language detection | ✅ Solid |
| Link graph / PageRank | ✅ Solid |
| Incremental crawling | ✅ Solid |
| Meilisearch integration | ✅ Solid |
| AI enrichment (OpenAI) | ⚠️ Exists but not exposed |

The core crawling, parsing, and storage layers are production-ready. What's missing is the **developer-friendly API layer** that made Firecrawl successful.

---

## Critical Missing Features

### 0. Meilisearch Index Sync Endpoint

**Priority: HIGH (Differentiator)**

Direct integration with Meilisearch Cloud for full website indexing. This is a **key differentiator** - Firecrawl doesn't offer native search engine integration.

**Current State:**
- Meilisearch indexing exists in content worker
- Tied to the job/crawl system
- No dedicated endpoint for "index this site to Meilisearch"
- No multi-index support in single request
- No way to bring your own Meilisearch instance easily

**What to Build:**

#### Single Site → Single Index

Uses the same configuration schema as existing `/crawl` endpoint, maintaining API consistency.

```
POST /index
{
  // === REQUIRED ===
  "start_urls": ["https://docs.example.com"],
  "meilisearch_url": "https://cloud.meilisearch.com",
  "meilisearch_api_key": "ms_xxx",
  "meilisearch_index_uid": "docs-example",

  // === INDEXING MODE ===
  "index_mode": "full",              // "full" | "incremental" | "append" | "diff"
  "incremental_since": null,         // ISO timestamp for incremental mode

  // === CRAWLER TYPE ===
  "crawler_type": "cheerio",         // "cheerio" | "puppeteer" | "playwright"

  // === URL CONTROL ===
  "urls_to_exclude": ["**/admin/**", "**/login"],
  "urls_to_index": ["**/docs/**", "**/guide/**"],
  "urls_to_not_index": ["**/changelog"],
  "use_sitemap": true,
  "sitemap_urls": ["https://docs.example.com/sitemap.xml"],

  // === PERFORMANCE ===
  "max_concurrency": 10,
  "max_requests_per_minute": 60,
  "max_request_retries": 3,
  "batch_size": 1000,
  "max_pages": 500,
  "max_depth": 10,

  // === FEATURES (same as /crawl) ===
  "features": {
    "metadata": {
      "activated": true
    },
    "markdown": {
      "activated": true
    },
    "schema": {
      "activated": true,
      "only_type": "Article",
      "convert_dates": true
    },
    "block_split": {
      "activated": true,
      "levels": ["h2", "h3"],
      "include_parent_context": true
    },
    "ai_extraction": {
      "activated": false,
      "prompt": "Extract key concepts"
    },
    "ai_summary": {
      "activated": false
    },
    "embeddings": {
      "activated": true,
      "model": "text-embedding-3-small",
      "dimensions": 1536
    }
  },

  // === MEILISEARCH SETTINGS ===
  "primary_key": "uid",
  "meilisearch_settings": {
    "searchableAttributes": ["title", "content", "markdown", "h1", "h2", "h3"],
    "filterableAttributes": ["domain", "language", "urls_tags", "path"],
    "sortableAttributes": ["crawled_at", "published_at"],
    "distinctAttribute": "url",
    "embedders": {
      "default": {
        "source": "userProvided",
        "dimensions": 1536
      }
    }
  },

  // === WEBHOOKS (same format as /crawl) ===
  "webhooks": {
    "main": {
      "url": "https://your-server.com/webhook",
      "auth": { "bearer": "token" },
      "events": ["run.completed", "run.failed"],
      "enabled": true
    }
  },

  // === GLOBAL INDEX (optional) ===
  "global_index": {
    "enabled": true,
    "index_uid": "scrapix-global"
  },

  // === REQUEST OPTIONS ===
  "additional_request_headers": { "Authorization": "Bearer token" },
  "user_agents": ["ScrapixBot/1.0"],

  // === ERROR DETECTION ===
  "not_found_selectors": [".error-404", "h1:contains('Not Found')"]
}
```

Response:
```json
{
  "job_id": "idx_abc123",
  "status": "running",
  "index_uid": "docs-example",
  "status_url": "/job/idx_abc123/status",
  "events_url": "/job/idx_abc123/events",
  "estimated_pages": 150
}
```

#### Multiple Sites → Multiple Indexes (Batch)
```
POST /index/batch
{
  // Shared Meilisearch connection
  "meilisearch_url": "https://cloud.meilisearch.com",
  "meilisearch_api_key": "ms_xxx",

  // Array of index jobs
  "indexes": [
    {
      "start_urls": ["https://docs.company.com"],
      "meilisearch_index_uid": "company-docs",
      "max_pages": 1000,
      "features": {
        "markdown": { "activated": true },
        "block_split": { "activated": true }
      }
    },
    {
      "start_urls": ["https://blog.company.com"],
      "meilisearch_index_uid": "company-blog",
      "max_pages": 200,
      "features": {
        "metadata": { "activated": true },
        "ai_summary": { "activated": true }
      }
    },
    {
      "start_urls": ["https://help.company.com"],
      "meilisearch_index_uid": "company-help",
      "max_pages": 500,
      "crawler_type": "puppeteer"
    }
  ],

  // Defaults applied to all indexes
  "default_options": {
    "crawler_type": "cheerio",
    "max_concurrency": 5,
    "features": {
      "metadata": { "activated": true },
      "embeddings": { "activated": true }
    }
  }
}
```

Response:
```json
{
  "batch_id": "batch_xyz789",
  "jobs": [
    { "job_id": "idx_001", "index_uid": "company-docs", "status": "queued" },
    { "job_id": "idx_002", "index_uid": "company-blog", "status": "queued" },
    { "job_id": "idx_003", "index_uid": "company-help", "status": "queued" }
  ],
  "status_url": "/index/batch/batch_xyz789/status"
}
```

#### Index Modes

| Mode | Behavior |
|------|----------|
| `full` | Delete existing index, create temp index, swap atomically on completion |
| `incremental` | Only crawl pages changed since `incremental_since` (ETags, Last-Modified) |
| `append` | Add new pages to existing index without touching existing documents |
| `diff` | Crawl all pages, compute diff, only update changed documents |

**Atomic Index Swap (inherited from current Scrapix):**
```
Start Index Job
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
    │       └── No: Keep original index (rollback)
    │
    ▼
Done (zero search downtime)
```

#### Index Status & Management

```
GET /job/{job_id}/status
```

Response (extended for index jobs):
```json
{
  "job_id": "idx_abc123",
  "status": "running",
  "started_at": "2024-01-15T10:30:00Z",
  "progress": {
    "pages_discovered": 234,
    "pages_crawled": 156,
    "pages_indexed": 142,
    "pages_failed": 3,
    "documents_created": 890,
    "crawl_rate": 12.5
  },
  "meilisearch": {
    "index_uid": "docs-example",
    "task_uid": 12345,
    "documents_count": 890,
    "index_size": "45.2 MB"
  },
  "eta": "2024-01-15T10:35:00Z"
}
```

```
GET /job/{job_id}/events
```
SSE stream (same as `/crawl`):
```
event: crawler.event
data: {"type": "PAGE_INDEXED", "url": "https://...", "documents": 3}

event: batch.processed
data: {"batch_size": 100, "batch_number": 5}

event: progress
data: {"crawled": 156, "indexed": 142, "rate": 12.5}
```

```
DELETE /job/{job_id}
```
Cancels indexing job. If using temp index, deletes temp index (no changes to production).

#### Scheduled Re-indexing

```
POST /index/schedule
{
  "name": "docs-daily-sync",
  "start_urls": ["https://docs.example.com"],
  "meilisearch_url": "https://cloud.meilisearch.com",
  "meilisearch_api_key": "ms_xxx",
  "meilisearch_index_uid": "docs-example",

  "schedule": {
    "cron": "0 2 * * *",
    "timezone": "UTC"
  },

  "index_mode": "incremental",
  "max_pages": 500,

  "features": {
    "markdown": { "activated": true },
    "embeddings": { "activated": true }
  },

  "webhooks": {
    "on_complete": {
      "url": "https://your-server.com/reindex-complete",
      "events": ["run.completed", "run.failed"]
    }
  }
}
```

Response:
```json
{
  "schedule_id": "sched_abc123",
  "name": "docs-daily-sync",
  "cron": "0 2 * * *",
  "next_run": "2024-01-16T02:00:00Z",
  "status": "active"
}
```

```
GET /index/schedules
DELETE /index/schedule/{schedule_id}
POST /index/schedule/{schedule_id}/trigger   # Run now
```

#### Document Schema for Meilisearch

**FullPageDocument** (when `block_split.activated: false`):
```json
{
  "uid": "uuid-v4",
  "url": "https://docs.example.com/guide/intro",
  "domain": "docs.example.com",
  "title": "Introduction Guide",
  "urls_tags": ["guide", "intro"],
  "blocks": [
    { "h1": "Introduction Guide", "p": "Welcome to..." },
    { "h2": "Setup", "p": "First, install...", "anchor": "setup" },
    { "h3": "Prerequisites", "p": "You need...", "anchor": "prerequisites" }
  ],
  "metadata": {
    "description": "Getting started with...",
    "og:image": "https://...",
    "author": "John Doe"
  },
  "markdown": "# Introduction Guide\n\nWelcome to...",
  "schema": {
    "@type": "Article",
    "author": { "@type": "Person", "name": "John Doe" }
  },
  "ai_extraction": { "key_concepts": ["setup", "configuration"] },
  "ai_summary": "This guide covers...",
  "language": "en",
  "crawled_at": "2024-01-15T10:30:00Z",
  "_vectors": {
    "default": [0.123, 0.456, ...]
  }
}
```

**BlockDocument** (when `block_split.activated: true`):
```json
{
  "uid": "uuid-v4-block",
  "parent_document_id": "uuid-v4-parent",
  "page_block": 2,
  "url": "https://docs.example.com/guide/intro",
  "domain": "docs.example.com",
  "title": "Introduction Guide",
  "h1": "Introduction Guide",
  "h2": "Setup",
  "h3": "Prerequisites",
  "p": ["You need Node.js 18+", "Install dependencies with npm"],
  "anchor": "prerequisites",
  "urls_tags": ["guide", "intro"],
  "metadata": { ... },
  "language": "en",
  "crawled_at": "2024-01-15T10:30:00Z",
  "_vectors": {
    "default": [0.123, 0.456, ...]
  }
}
```

#### Meilisearch Settings Configuration

```json
{
  "meilisearch_settings": {
    "searchableAttributes": [
      "title",
      "h1", "h2", "h3", "h4", "h5", "h6",
      "p",
      "markdown",
      "metadata.description"
    ],
    "filterableAttributes": [
      "domain",
      "language",
      "urls_tags",
      "parent_document_id"
    ],
    "sortableAttributes": [
      "crawled_at"
    ],
    "rankingRules": [
      "words",
      "typo",
      "proximity",
      "attribute",
      "sort",
      "exactness"
    ],
    "distinctAttribute": "url",
    "embedders": {
      "default": {
        "source": "openAi",
        "model": "text-embedding-3-small",
        "apiKey": "sk-...",
        "documentTemplate": "{{doc.title}}\n\n{{doc.markdown}}"
      }
    }
  }
}
```

#### Embeddings Configuration

Two modes for vector generation:

**1. Scrapix-generated (pre-computed):**
```json
{
  "features": {
    "embeddings": {
      "activated": true,
      "model": "text-embedding-3-small",
      "dimensions": 1536,
      "field": "markdown"
    }
  },
  "meilisearch_settings": {
    "embedders": {
      "default": {
        "source": "userProvided",
        "dimensions": 1536
      }
    }
  }
}
```

**2. Meilisearch-generated (on indexing):**
```json
{
  "features": {
    "embeddings": { "activated": false }
  },
  "meilisearch_settings": {
    "embedders": {
      "default": {
        "source": "openAi",
        "model": "text-embedding-3-small",
        "apiKey": "sk-...",
        "documentTemplate": "{{doc.title}}: {{doc.markdown}}"
      }
    }
  }
}
```

**Implementation:**

1. New `/index` endpoint reuses existing crawl pipeline
2. Same config schema as `/crawl` with additional `index_mode`, `global_index` fields
3. Accepts external Meilisearch credentials (overrides env vars)
4. Progress tracking via existing SSE `/job/{id}/events`
5. Atomic index swap (already implemented in Scrapix)
6. Schedule storage in Redis with cron evaluation
7. Batch endpoint creates multiple jobs with shared connection

**Why This Is a Differentiator:**

- Firecrawl outputs data, you figure out where to put it
- Scrapix: **One API call → Searchable Meilisearch index**
- Uses familiar Scrapix config schema (no new API to learn)
- Atomic swaps = zero search downtime
- Scheduled re-indexing keeps content fresh
- Block splitting optimized for RAG/AI search
- Batch indexing for multi-site deployments

---

### 1. Simple Single-URL Scrape Endpoint

**Priority: HIGH**

Firecrawl's killer feature is `POST /scrape` - instant, synchronous, single-URL scraping that returns clean markdown in ~1 second.

**Current State:**
- Scrapix forces everything through the job/queue system
- No instant scrape without Kafka overhead
- Simplest operation requires creating a job, polling status, fetching results

**What to Build:**
```
POST /scrape
{
  "url": "https://example.com/page",
  "formats": ["markdown", "html", "links"],
  "onlyMainContent": true,
  "waitFor": 2000
}
```

Response (synchronous, <2s):
```json
{
  "url": "https://example.com/page",
  "markdown": "# Page Title\n\nContent here...",
  "html": "<article>...</article>",
  "links": ["https://example.com/other", "..."],
  "metadata": {
    "title": "Page Title",
    "description": "...",
    "language": "en"
  }
}
```

**Implementation:**
- Add fast-path that bypasses Kafka for single URLs
- Direct fetch → parse → return pipeline
- Use response cache for repeated requests
- Timeout at 30s max

---

### 2. Flexible Output Formats

**Priority: HIGH**

Firecrawl lets you request multiple formats in one call: `formats: ["markdown", "html", "links", "screenshot", "summary"]`

**Current State:**
- Document output is fixed
- Screenshots exist in renderer but not exposed via API
- No format selection parameter
- No on-demand summary generation

**What to Build:**

Supported formats:
| Format | Description |
|--------|-------------|
| `markdown` | Clean markdown content (default) |
| `html` | Cleaned HTML (main content only) |
| `rawHtml` | Original HTML as fetched |
| `links` | Array of URLs found on page |
| `images` | Array of image URLs |
| `screenshot` | Base64 PNG of rendered page |
| `screenshot@fullPage` | Full-page screenshot |
| `summary` | AI-generated summary |
| `metadata` | Extracted metadata object |
| `schema` | JSON-LD/microdata structured data |

**Implementation:**
- Add `formats` array parameter to `/scrape` and `/crawl`
- Conditionally run extraction based on requested formats
- Screenshot requires browser rendering path
- Summary requires AI enrichment path

---

### 3. LLM-Powered Structured Extraction

**Priority: HIGH**

Firecrawl's "zero-selector" extraction: pass a prompt + JSON schema, get structured data back.

**Current State:**
- `scrapix-ai` crate has `AiExtractor` with schema support
- Not exposed via any API endpoint
- Requires writing Rust code to use

**What to Build:**

```
POST /extract
{
  "urls": ["https://example.com/product/*"],
  "prompt": "Extract product information",
  "schema": {
    "type": "object",
    "properties": {
      "name": { "type": "string" },
      "price": { "type": "number" },
      "currency": { "type": "string" },
      "inStock": { "type": "boolean" }
    },
    "required": ["name", "price"]
  }
}
```

Response:
```json
{
  "data": [
    {
      "url": "https://example.com/product/123",
      "extraction": {
        "name": "Widget Pro",
        "price": 29.99,
        "currency": "USD",
        "inStock": true
      }
    }
  ]
}
```

**Implementation:**
- New `/extract` endpoint (async for multiple URLs)
- Support wildcard URLs that trigger crawl
- JSON Schema validation on output
- Batch processing with rate limiting
- Token budget management

---

### 4. Browser Actions System

**Priority: HIGH**

Firecrawl can click, scroll, fill forms, and execute JS before scraping.

**Current State:**
- CDP renderer exists with full browser control
- No action choreography exposed
- Can't specify interactions via API

**What to Build:**

```
POST /scrape
{
  "url": "https://example.com/spa",
  "actions": [
    { "type": "wait", "milliseconds": 1000 },
    { "type": "click", "selector": "button.load-more" },
    { "type": "wait", "milliseconds": 2000 },
    { "type": "scroll", "direction": "down", "amount": 500 },
    { "type": "fill", "selector": "input[name=search]", "value": "query" },
    { "type": "press", "key": "Enter" },
    { "type": "waitForSelector", "selector": ".results" },
    { "type": "screenshot" }
  ]
}
```

Supported actions:
| Action | Parameters |
|--------|------------|
| `wait` | `milliseconds` |
| `waitForSelector` | `selector`, `timeout` |
| `click` | `selector` |
| `fill` | `selector`, `value` |
| `press` | `key` (Enter, Tab, Escape, etc.) |
| `scroll` | `direction`, `amount` or `selector` |
| `screenshot` | `fullPage`, `selector` |
| `evaluate` | `script` (JS to execute) |

**Implementation:**
- Action executor in crawler using chromiumoxide
- Sequential execution with error handling
- Timeout per action and total
- Return screenshot if action requests it

---

### 5. Hybrid Search (Meilisearch-First + External Fallback)

**Priority: HIGH (Critical for Exa.ai parity)**

**Key Insight:** Build your own search index, use external search as fallback. This:
- Reduces external API costs over time
- Builds a proprietary index (competitive moat)
- Provides faster responses for indexed content
- Allows semantic/vector search on your own data

**Current State:**
- No search capability
- Must provide exact URLs to crawl
- Meilisearch stores crawled content but no search API exposed

**What to Build:**

#### Search Flow Architecture

```
                         POST /search
                              │
                              ▼
                    ┌─────────────────┐
                    │  Query Router   │
                    └─────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        ┌──────────┐   ┌──────────┐   ┌──────────┐
        │Meilisearch│   │Meilisearch│   │ External │
        │ Keyword  │   │  Vector  │   │  Search  │
        └──────────┘   └──────────┘   └──────────┘
              │               │               │
              └───────────────┼───────────────┘
                              ▼
                    ┌─────────────────┐
                    │  Result Merger  │
                    │  + Deduplication│
                    └─────────────────┘
                              │
                              ▼
                    ┌─────────────────┐
                    │ Optional Scrape │
                    │ (if not cached) │
                    └─────────────────┘
                              │
                              ▼
                         Response
```

#### API Design

```
POST /search
{
  "query": "rust web crawler libraries",
  "limit": 10,
  "sources": {
    "internal": {
      "enabled": true,
      "index": "global-index",
      "searchType": "hybrid",
      "vectorWeight": 0.5
    },
    "external": {
      "enabled": true,
      "provider": "brave",
      "fallbackOnly": true
    }
  },
  "scrapeResults": true,
  "formats": ["markdown", "links"],
  "filters": {
    "domains": ["github.com", "docs.rs"],
    "excludeDomains": ["reddit.com"],
    "language": "en",
    "publishedAfter": "2024-01-01"
  }
}
```

Response:
```json
{
  "results": [
    {
      "url": "https://docs.rs/spider/latest",
      "title": "spider - Rust Web Crawler",
      "snippet": "A fast web crawler written in Rust...",
      "source": "internal",
      "score": 0.95,
      "markdown": "# spider\n\nA fast web crawler...",
      "links": [...],
      "cached": true,
      "indexedAt": "2024-01-10T00:00:00Z"
    },
    {
      "url": "https://blog.example.com/rust-crawlers",
      "title": "Best Rust Web Crawlers in 2024",
      "snippet": "A comparison of...",
      "source": "external:brave",
      "score": 0.82,
      "markdown": "# Best Rust Web Crawlers\n...",
      "cached": false,
      "scrapedAt": "2024-01-15T10:30:00Z"
    }
  ],
  "meta": {
    "internalResults": 3,
    "externalResults": 7,
    "fromCache": 4,
    "freshlyScraped": 6
  }
}
```

#### Search Modes

| Mode | Behavior |
|------|----------|
| `internal-only` | Only search Meilisearch index |
| `external-only` | Only use external search (Brave, etc.) |
| `hybrid` | Search both, merge results |
| `fallback` | Internal first, external only if insufficient results |

#### Search Types (Internal)

| Type | Description |
|------|-------------|
| `keyword` | Traditional full-text search |
| `semantic` | Vector similarity search |
| `hybrid` | Combined keyword + vector with configurable weight |

#### Global Index Strategy

Maintain a **global Meilisearch index** that accumulates all crawled content:

```
POST /index
{
  "url": "https://docs.example.com",
  "meilisearch": {
    "url": "https://cloud.meilisearch.com",
    "apiKey": "ms_xxx",
    "indexUid": "docs-example"
  },
  "globalIndex": {
    "enabled": true,
    "indexUid": "scrapix-global"
  }
}
```

Every crawl optionally contributes to the global index, building up searchable content over time.

#### Auto-Index on Search

When external search returns results, automatically index them:

```json
{
  "autoIndex": {
    "enabled": true,
    "indexUid": "scrapix-global",
    "onlyIfScraped": true
  }
}
```

This means:
1. User searches for "rust web crawlers"
2. Meilisearch returns 2 results from previous crawls
3. Brave returns 8 more results
4. Scrapix scrapes those 8 pages
5. Those 8 pages are added to the global index
6. Next search for similar query → more internal results

**The index grows organically with usage.**

#### External Search Providers

| Provider | Cost | Quality | Notes |
|----------|------|---------|-------|
| Brave Search API | $5/1k queries | Good | Privacy-focused, good for general search |
| SerpAPI | $50/5k queries | Excellent | Google results, expensive |
| Bing Web Search | $7/1k queries | Good | Microsoft, reasonable |
| Tavily | $0.01/query | Good | Built for AI, includes content |
| Self-hosted SearXNG | Free | Variable | Requires infrastructure |

**Recommended:** Start with Brave Search API, add Tavily for AI-specific queries.

#### Why This Matters

1. **Cost Efficiency**: External search costs money. Internal search is essentially free.
2. **Speed**: Meilisearch responses are <50ms. External search + scrape is 2-5s.
3. **Moat**: Over time, your index becomes valuable proprietary data.
4. **Quality**: You control what's indexed and how it's ranked.
5. **Exa.ai Parity**: This is exactly how Exa works - own index + fallback.

---

### 6. Map Endpoint

**Priority: MEDIUM**

Fast URL discovery without full content scraping.

**Current State:**
- Must do full crawl to get URL list
- No lightweight discovery mode

**What to Build:**

```
POST /map
{
  "url": "https://docs.example.com",
  "limit": 1000,
  "search": "api reference"
}
```

Response:
```json
{
  "urls": [
    {
      "url": "https://docs.example.com/api/users",
      "title": "Users API Reference",
      "description": "Endpoints for user management"
    },
    ...
  ]
}
```

**Implementation:**
- Lightweight crawl that only fetches HTML, extracts links
- Optional sitemap parsing for faster results
- Keyword filtering on URL/title
- Return within seconds, not minutes

---

### 7. Agent Mode

**Priority: MEDIUM (Differentiator)**

Autonomous research agent that takes natural language prompts.

**Current State:**
- AI crate exists but no orchestration
- No autonomous decision-making

**What to Build:**

```
POST /agent
{
  "prompt": "Find the founders of Anthropic, their backgrounds, and when they founded the company",
  "schema": {
    "type": "object",
    "properties": {
      "company": { "type": "string" },
      "founded": { "type": "string" },
      "founders": {
        "type": "array",
        "items": {
          "type": "object",
          "properties": {
            "name": { "type": "string" },
            "role": { "type": "string" },
            "background": { "type": "string" }
          }
        }
      }
    }
  }
}
```

**Implementation:**
- LLM-powered orchestrator that decides:
  - What to search for
  - Which URLs to scrape
  - What actions to take on pages
  - When enough data is gathered
- Uses `/search`, `/scrape`, `/extract` internally
- Returns structured data matching schema
- Includes sources/citations

---

### 8. Response Caching

**Priority: MEDIUM**

**Current State:**
- No global response cache
- Every request re-fetches the URL
- No `maxAge` parameter

**What to Build:**

```
POST /scrape
{
  "url": "https://example.com",
  "maxAge": 3600
}
```

- Cache key: URL + relevant options hash
- Default TTL: 48 hours (configurable)
- `maxAge: 0` forces fresh fetch
- Cache backends: Redis (primary), local (fallback)

**Implementation:**
- Redis-based cache with LRU eviction
- Store: URL → (content, metadata, timestamp)
- Check cache before fetch
- Background refresh for popular URLs

---

### 9. Webhooks Implementation

**Priority: MEDIUM**

**Current State:**
- Configuration schema exists in `CrawlConfig`
- No actual webhook dispatcher
- No event delivery

**What to Build:**

Configuration:
```json
{
  "webhook": {
    "url": "https://your-server.com/webhook",
    "events": ["crawl.started", "crawl.page", "crawl.completed", "crawl.failed"],
    "secret": "your-hmac-secret",
    "metadata": { "customId": "abc123" }
  }
}
```

Webhook payload:
```json
{
  "event": "crawl.page",
  "jobId": "job_xxx",
  "timestamp": "2024-01-15T10:30:00Z",
  "data": {
    "url": "https://...",
    "status": 200
  },
  "metadata": { "customId": "abc123" }
}
```

**Implementation:**
- Webhook dispatcher service
- HMAC-SHA256 signature in `X-Scrapix-Signature` header
- Exponential backoff retries (3 attempts)
- Event filtering per webhook
- Async delivery (don't block crawl)

---

### 10. SDKs & Integrations

**Priority: HIGH**

**Current State:**
- CLI only
- No language SDKs
- No framework integrations

**What to Build:**

#### Python SDK (`scrapix-py`)
```python
from scrapix import Scrapix

client = Scrapix(api_key="sk_...")

# Simple scrape
result = client.scrape("https://example.com", formats=["markdown", "links"])
print(result.markdown)

# Crawl with options
job = client.crawl(
    url="https://docs.example.com",
    max_pages=100,
    formats=["markdown"]
)
for page in job.stream():
    print(page.url)

# Extract structured data
data = client.extract(
    urls=["https://example.com/products/*"],
    prompt="Extract product name and price",
    schema=ProductSchema
)
```

#### Node.js SDK (`@scrapix/sdk`)
```typescript
import { Scrapix } from '@scrapix/sdk';

const client = new Scrapix({ apiKey: 'sk_...' });

const result = await client.scrape('https://example.com', {
  formats: ['markdown', 'links']
});

console.log(result.markdown);
```

#### LangChain Integration
```python
from langchain_community.document_loaders import ScrapixLoader

loader = ScrapixLoader(
    url="https://docs.example.com",
    api_key="sk_...",
    max_pages=50
)
docs = loader.load()
```

#### MCP Server
For integration with Claude, Cursor, and other AI tools:
```json
{
  "mcpServers": {
    "scrapix": {
      "command": "scrapix-mcp",
      "args": ["--api-key", "sk_..."]
    }
  }
}
```

---

### 11. Authentication & API Keys

**Priority: MEDIUM**

**Current State:**
- Optional API key via env var
- No key management
- No per-key rate limiting

**What to Build:**
- API key generation and management
- Per-key usage tracking
- Rate limiting per key
- Key scopes (read-only, full access)
- Usage dashboard

---

### 12. Anti-Bot & Proxy Infrastructure

**Priority: MEDIUM**

**Current State:**
- Basic proxy rotation exists
- No fingerprint randomization
- No CAPTCHA solving

**What to Build:**
- User-agent rotation pool
- Browser fingerprint randomization
- Request header variation
- Optional CAPTCHA solving integration (2captcha, hCaptcha)
- Residential proxy support
- Geotargeting by country

---

## Exa.ai-Specific Features

To compete with Exa.ai specifically (semantic search for AI), leverage **Meilisearch's hybrid search** as the foundation.

### How Exa.ai Works

Exa has:
- **Pre-built index** of billions of web pages
- **Neural search** ranking (embeddings, not just keywords)
- **Similarity search** ("find pages like this URL")
- **Autoprompt** (query enhancement for better results)
- **Contents API** (fetch full content for URLs)

### Scrapix Strategy: Meilisearch-Powered Exa

Instead of building a separate vector database, use **Meilisearch's native hybrid search**:

```
┌─────────────────────────────────────────────────────────┐
│                    Meilisearch Index                     │
├─────────────────────────────────────────────────────────┤
│  Documents with:                                         │
│  - Full-text content (keyword search)                   │
│  - _vectors field (semantic search)                     │
│  - Filterable: domain, language, date, path             │
│  - Sortable: crawledAt, publishedAt, score              │
└─────────────────────────────────────────────────────────┘
                           │
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
      Keyword Search  Vector Search   Hybrid Search
      (BM25-like)     (cosine sim)    (weighted combo)
```

### Neural/Semantic Search

```
POST /search
{
  "query": "how to fine-tune large language models",
  "searchType": "hybrid",
  "hybridConfig": {
    "semanticRatio": 0.7,
    "embedder": "default"
  },
  "limit": 10,
  "filters": {
    "domains": ["arxiv.org", "huggingface.co"],
    "excludeDomains": ["reddit.com"],
    "publishedAfter": "2024-01-01",
    "language": "en"
  },
  "sources": {
    "internal": { "enabled": true },
    "external": { "enabled": true, "fallbackOnly": true }
  }
}
```

**Meilisearch Hybrid Search Config:**
```json
{
  "embedders": {
    "default": {
      "source": "openAi",
      "model": "text-embedding-3-small",
      "dimensions": 1536
    }
  },
  "searchCutoffMs": 150
}
```

### Similarity Search

Find pages similar to a given URL using vector similarity:

```
POST /similar
{
  "url": "https://arxiv.org/abs/2401.12345",
  "limit": 10,
  "filters": {
    "excludeDomains": ["arxiv.org"],
    "publishedAfter": "2023-01-01"
  }
}
```

**Implementation:**
1. Look up URL in index, get its embedding vector
2. If not indexed, scrape and embed it first
3. Use Meilisearch vector search with that embedding
4. Return similar documents

```
POST /similar
{
  "text": "Transformer architectures for language understanding...",
  "limit": 10
}
```

Also support raw text input (embed on the fly).

### Highlight Extraction

Return relevant snippets instead of full content:

```
POST /search
{
  "query": "fine-tuning methods for LLMs",
  "highlights": {
    "enabled": true,
    "numSentences": 3,
    "maxLength": 500
  },
  "returnContent": false
}
```

Response:
```json
{
  "results": [
    {
      "url": "https://...",
      "title": "A Guide to Fine-tuning LLMs",
      "highlights": [
        "LoRA (Low-Rank Adaptation) has become the most popular fine-tuning method due to its efficiency.",
        "QLoRA combines quantization with LoRA to enable fine-tuning on consumer GPUs.",
        "Full fine-tuning remains the gold standard for maximum performance but requires significant compute."
      ],
      "score": 0.94
    }
  ]
}
```

**Implementation:** Use Meilisearch's `_matchesPosition` + surrounding sentence extraction.

### Contents API (Exa Compatibility)

Fetch content for known URLs (like Exa's `/contents`):

```
POST /contents
{
  "urls": [
    "https://arxiv.org/abs/2401.12345",
    "https://huggingface.co/blog/peft"
  ],
  "formats": ["markdown", "summary"],
  "highlights": {
    "query": "fine-tuning efficiency",
    "numSentences": 2
  }
}
```

- Check Meilisearch index first (instant if cached)
- Scrape if not found
- Optionally add to global index

### Building the Global Index

For true Exa.ai competition, proactively build an index:

#### Seed Sources (High-Value Domains)

| Category | Domains |
|----------|---------|
| Research | arxiv.org, scholar.google.com, semanticscholar.org |
| Documentation | docs.*, readthedocs.io, gitbook.io |
| Code | github.com, gitlab.com, docs.rs, pkg.go.dev |
| News/Tech | news.ycombinator.com, techcrunch.com, wired.com |
| Reference | wikipedia.org, stackoverflow.com |

#### Proactive Crawl Strategy

```
POST /index/schedule
{
  "name": "arxiv-daily",
  "url": "https://arxiv.org/list/cs.AI/recent",
  "meilisearch": {
    "indexUid": "scrapix-global"
  },
  "schedule": {
    "cron": "0 6 * * *",
    "timezone": "UTC"
  },
  "options": {
    "maxPages": 100,
    "generateEmbeddings": true
  }
}
```

#### Index Growth Flywheel

```
User searches → External results scraped → Added to index
                                              ↓
                        Next search → More internal results
                                              ↓
                              Less external API usage
                                              ↓
                                    Lower costs, faster responses
```

### Autoprompt (Query Enhancement)

Use LLM to improve search queries:

```
POST /search
{
  "query": "how do I make my model better",
  "autoprompt": true
}
```

Internally transforms to: "techniques for improving machine learning model performance accuracy fine-tuning hyperparameter optimization"

**Implementation:** Quick LLM call to expand/clarify query before search.

### Meilisearch Settings for Exa-like Search

```json
{
  "searchableAttributes": [
    "title",
    "content",
    "description",
    "headings"
  ],
  "filterableAttributes": [
    "domain",
    "language",
    "publishedAt",
    "crawledAt",
    "path",
    "contentType"
  ],
  "sortableAttributes": [
    "publishedAt",
    "crawledAt",
    "_rankingScore"
  ],
  "rankingRules": [
    "words",
    "typo",
    "proximity",
    "attribute",
    "sort",
    "exactness"
  ],
  "embedders": {
    "default": {
      "source": "openAi",
      "model": "text-embedding-3-small",
      "documentTemplate": "{{doc.title}}\n\n{{doc.content}}"
    }
  }
}
```

---

## Implementation Priority Roadmap

### Phase 1: Meilisearch Integration & Core API (4-6 weeks)
**Goal:** Meilisearch-native indexing + basic Firecrawl parity

1. **`POST /index`** - Full website indexing to Meilisearch (key differentiator)
2. **`POST /index/batch`** - Multi-site, multi-index batch indexing
3. **`POST /scrape`** - Instant single-URL scrape
4. **Format selection** - markdown, html, links, screenshot
5. **Response caching** - Redis-based with TTL
6. **Incremental re-indexing** - ETags, Last-Modified support

### Phase 2: Extraction & Developer Experience (4-6 weeks)
**Goal:** LLM features + SDK adoption

7. **`POST /extract`** - LLM extraction with JSON schema
8. **Python SDK** - `pip install scrapix`
9. **Node.js SDK** - `npm install @scrapix/sdk`
10. **Chunk mode** - Split pages by headings for RAG
11. **Embedding generation** - Vector indexing in Meilisearch
12. **Webhooks** - Event delivery with HMAC signatures

### Phase 3: Advanced Features (4-6 weeks)
**Goal:** Full Firecrawl feature parity

13. **Browser actions** - click, scroll, fill, wait
14. **`POST /search`** - Web search integration (Brave API)
15. **`POST /map`** - Fast URL discovery
16. **Scheduled re-indexing** - Cron-based index refresh
17. **LangChain loader** - Document loader integration
18. **MCP server** - AI tool integration (Claude, Cursor)

### Phase 4: AI Agent & Platform (4-6 weeks)
**Goal:** Autonomous capabilities + production platform

19. **`POST /agent`** - Autonomous research agent
20. **API key management** - Multi-tenant authentication
21. **Usage dashboard** - Metrics, billing, quotas
22. **Rate limiting** - Per-key limits

### Phase 5: Exa.ai Competition (6-8 weeks)
**Goal:** Semantic search capabilities

23. **Persistent search index** - Proactive crawling of high-value sites
24. **Neural search** - Hybrid keyword + vector ranking
25. **Similarity search** - Find pages similar to a URL
26. **Highlight extraction** - Smart snippets for search results
27. **Domain/date filtering** - Advanced search parameters

---

## Architecture Considerations

### Fast Path vs Distributed Path

```
                    ┌─────────────────────────────────────┐
                    │           API Server                │
                    └─────────────────────────────────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    ▼                               ▼
            ┌───────────────┐               ┌───────────────┐
            │   Fast Path   │               │ Distributed   │
            │  (single URL) │               │    Path       │
            └───────────────┘               └───────────────┘
                    │                               │
                    ▼                               ▼
            ┌───────────────┐               ┌───────────────┐
            │ Direct Fetch  │               │    Kafka      │
            │  + Parse      │               │   Queues      │
            └───────────────┘               └───────────────┘
                    │                               │
                    ▼                               ▼
            ┌───────────────┐               ┌───────────────┐
            │ Response      │               │   Workers     │
            │   Cache       │               │ (distributed) │
            └───────────────┘               └───────────────┘
```

- `/scrape` uses Fast Path (sync, <30s)
- `/crawl`, `/extract` (multi-URL) use Distributed Path (async)
- Cache serves both paths

### New Crate Structure

```
crates/
  scrapix-sdk/           # Shared SDK types (for codegen)
  scrapix-actions/       # Browser action executor
  scrapix-search/        # Search provider integrations
  scrapix-agent/         # Autonomous agent orchestrator
  scrapix-cache/         # Response caching layer
  scrapix-webhook/       # Webhook dispatcher
```

---

## Success Metrics

### Firecrawl Parity
- [ ] `/scrape` returns in <2s for cached, <10s for fresh
- [ ] All format options supported
- [ ] Browser actions work for SPAs
- [ ] Python SDK published to PyPI
- [ ] Can extract structured data with schema

### Exa.ai Competition
- [ ] Semantic search returns relevant results
- [ ] Index covers 1M+ pages
- [ ] Similarity search works
- [ ] Sub-second search response times

### Developer Adoption
- [ ] SDK downloads >1000/month
- [ ] LangChain integration in docs
- [ ] MCP server available

---

## Conclusion

Scrapix has the **hard parts done** - distributed crawling, deduplication, parsing, and storage are solid. The gap is in the **API layer and developer experience**:

| What Scrapix Has | What's Missing |
|------------------|----------------|
| Distributed crawler | Simple `/scrape` endpoint |
| JS rendering | Browser actions API |
| AI extraction (internal) | `/extract` endpoint |
| Meilisearch storage | `/index` endpoint for direct indexing |
| CLI tool | SDKs (Python, Node.js) |

### Key Differentiator: Native Meilisearch Integration

**Firecrawl gives you data. Scrapix gives you a searchable index.**

While Firecrawl outputs markdown/JSON that you must then process and store yourself, Scrapix can be positioned as **"the official web indexer for Meilisearch"**:

```
One API call: URL → Searchable Meilisearch Index
```

This is compelling for:
- **Documentation search** - Index your docs site in minutes
- **Site search** - Add search to any website
- **Knowledge bases** - Build RAG systems with chunked, embedded content
- **Multi-site aggregation** - Index multiple sources into one search

The path to competition:
1. **Lead with Meilisearch integration** - `/index` as the flagship feature
2. **Add Firecrawl-compatible APIs** - `/scrape`, `/extract`, `/map`
3. **Build SDKs** for developer adoption
4. **Integrate web search** for Exa.ai parity

Most features are **API/integration work**, not fundamental architecture changes. The infrastructure is ready.
