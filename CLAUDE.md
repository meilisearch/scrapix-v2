# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Workflow: Linear Issue Tracking

When creating a plan or proposing any major addition/feature to the project, **always create a Linear issue first** using the Linear MCP tool:
- **Team:** SCR (`https://linear.app/meilisearch/team/SCR/`)
- **Project:** "Console — Yet Another Meilisearch UI" (`https://linear.app/meilisearch/project/console-yet-another-meilisearch-ui-8f6681d804f7`)

The issue should contain the plan summary, scope of changes, and affected files. Do this before starting implementation.

## Pre-Commit Checks

Before every commit, **always** run these commands in order and fix any issues:

```bash
cargo fmt
cargo check
cargo clippy
```

All three must pass with no errors before committing.

## Project Overview

Scrapix is a high-performance, distributed web crawler and search indexer built in Rust. It's designed for internet-scale crawling with three main use cases: global internet indexing, targeted site crawling, and real-time information retrieval.

## Build Commands

```bash
# Build all crates
cargo build

# Build for production (with LTO, single codegen unit)
cargo build --release

# Build specific binary
cargo build --bin scrapix-api
cargo build --bin scrapix-worker-crawler
cargo build --bin scrapix-worker-content
cargo build --bin scrapix-frontier-service
cargo build --bin scrapix-cli

# Check without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

## Testing

```bash
# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p scrapix-core
cargo test -p scrapix-parser
cargo test -p scrapix-frontier

# Run a specific integration test
cargo test --test parser_extractor
cargo test --test frontier
cargo test --test crawl_pipeline
cargo test --test incremental_crawling
cargo test --test link_graph
cargo test --test dns_cache

# Run benchmarks
cargo bench -p scrapix-benchmarks
cargo bench --bench integrated_benchmarks
cargo bench --bench wikipedia_e2e
```

## Running Locally (Recommended)

**Prerequisites:** `just`, `overmind`, `tmux`, `cargo-watch` (all via Homebrew)

```bash
# Start everything — infrastructure + all services + console
just dev

# Or step by step:
just infra        # Start Docker infra (Redpanda, Meilisearch, DragonflyDB, Postgres, ClickHouse)
just services     # Start all Rust services + console via overmind

# Manage individual services
just logs api     # Attach to API service logs (overmind connect)
just restart api  # Restart just the API service
just stop         # Stop everything (services + infra)
```

**How it works:**
- Infrastructure runs in Docker (via `docker-compose.dev.yml` overlay which disables app services)
- Rust services run natively with `cargo-watch` — shared `target/` dir means one incremental build (~3-5s)
- Console runs natively with `npm run dev`
- All managed by overmind (tmux-based process manager)
- Environment loaded from `.env` via `set dotenv-load` in the justfile

**Individual service commands** (when you only need one):
```bash
just api       # cargo watch for scrapix-api only
just frontier  # cargo watch for frontier only
just crawler   # cargo watch for crawler only
just content   # cargo watch for content only
just console   # npm run dev for console only
```

Start a crawl:
```bash
scrapix crawl -p examples/simple-crawl.json
```

## Docker Compose

Docker Compose is available for full-stack containerized development, but `just dev` (native services) is faster for iteration.

```bash
# Full stack in containers with file watching
docker compose watch

# Infrastructure only (for use with `just services`)
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d

# Stop (preserves build caches in named volumes)
docker compose down

# Stop and remove all volumes (full reset)
docker compose down -v
```

## Diagnostic CLI Commands

Use these commands to quickly analyze system state during debugging (requires API server running):

```bash
# System-wide stats (jobs, domains tracked, success rate)
scrapix stats
scrapix stats -o json

# Recent errors with status codes and domain breakdown
scrapix errors --last 20
scrapix errors --job <job_id>
scrapix errors -o json

# Per-domain statistics (requests, success rate, avg latency)
scrapix domains --top 20
scrapix domains --filter wikipedia
scrapix domains -o json

# Check API health
scrapix health
```

**API endpoints (for programmatic access):**
- `GET /stats` - System stats (job counts, domain counters, error counts)
- `GET /errors?last=20&job_id=X` - Recent errors with distributions
- `GET /domains?top=20&filter=X` - Per-domain request stats

**Notes:**
- All diagnostic data is from in-memory tracking (recent only, since API startup)
- Error ring buffer holds last 1000 errors
- Domain counters are aggregated from crawl events

## Analytics API (Tinybird-style)

When ClickHouse is configured (`CLICKHOUSE_URL` environment variable), the API server automatically:
1. **Persists crawl events to ClickHouse** - PageCrawled and PageFailed events are batched (100 events) and flushed every 5 seconds
2. **Enables the analytics API** at `/analytics/v0/pipes/`

This provides long-term analytics storage beyond the in-memory diagnostics.

### CLI Commands

```bash
# List available analytics pipes
scrapix analytics pipes

# Key performance indicators
scrapix analytics kpis --hours 24

# Top domains by request count
scrapix analytics top-domains --hours 24 --limit 10

# Stats for a specific domain
scrapix analytics domain-stats --domain example.com --hours 24

# Hourly crawl statistics
scrapix analytics hourly --hours 24

# Error breakdown by status code
scrapix analytics error-dist --hours 24

# Job statistics
scrapix analytics job-stats --job-id abc123

# JSON output
scrapix analytics kpis -o json
```

### REST API

**List available pipes:**
```bash
curl http://localhost:8080/analytics/v0/pipes
```

**Available pipes:**
```bash
# Top domains by request count
curl "http://localhost:8080/analytics/v0/pipes/top_domains.json?hours=24&limit=10"

# Stats for a specific domain
curl "http://localhost:8080/analytics/v0/pipes/domain_stats.json?domain=example.com&hours=24"

# Hourly crawl statistics
curl "http://localhost:8080/analytics/v0/pipes/hourly_stats.json?hours=24"

# Error breakdown by status code
curl "http://localhost:8080/analytics/v0/pipes/error_distribution.json?hours=24"

# Job statistics
curl "http://localhost:8080/analytics/v0/pipes/job_stats.json?job_id=abc123"

# Key performance indicators
curl "http://localhost:8080/analytics/v0/pipes/kpis.json?hours=24"
```

**Response format (Tinybird-compatible):**
```json
{
  "meta": [{"name": "domain", "type": "String"}, ...],
  "data": [...],
  "rows": 10,
  "statistics": {"elapsed": 0.015, "rows_read": 10, "bytes_read": 0}
}
```

## Architecture

### Workspace Structure

The project is organized as a Cargo workspace with two main directories:

**Library Crates (`crates/`):**
- `scrapix-core` - Shared types, traits, configuration schemas, error types
- `scrapix-frontier` - URL frontier with bloom filter deduplication, priority scheduling, SimHash/MinHash near-duplicate detection
- `scrapix-crawler` - HTTP fetching (reqwest), JS rendering (chromiumoxide), robots.txt, DNS caching, proxy rotation
- `scrapix-parser` - HTML parsing (scraper), content extraction, markdown conversion, language detection
- `scrapix-extractor` - Feature extraction: metadata, JSON-LD/microdata schemas, custom CSS selectors, block splitting
- `scrapix-ai` - AI enrichment via OpenAI: extraction, summarization, embeddings
- `scrapix-storage` - Storage backends: Meilisearch, RocksDB, Redis/DragonflyDB, S3/MinIO, ClickHouse
- `scrapix-queue` - Kafka/Redpanda message queue producer and consumer
- `scrapix-telemetry` - Prometheus metrics, distributed tracing, structured logging

**Binary Crates (`bins/`):**
- `scrapix-api` - REST API server (axum) with WebSocket support
- `scrapix-worker-crawler` - Crawler worker that fetches URLs from the frontier
- `scrapix-worker-content` - Content processor that parses HTML and indexes to Meilisearch
- `scrapix-frontier-service` - Frontier service managing URL queue and deduplication
- `scrapix-cli` - CLI tool for starting crawls and checking status

**Frontend (`console/`):**
- Next.js 16 app (App Router) with TypeScript, Tailwind CSS v4, shadcn/ui
- Runs on port 3001 (`npm run dev`)
- `Dockerfile.dev` for containerized development with `docker compose watch`

### Data Flow

1. API receives crawl request → publishes to Redpanda
2. Frontier Service deduplicates URLs → assigns priorities → partitions by domain
3. Crawler Workers consume URLs → fetch pages → extract links → publish raw HTML
4. Content Workers consume HTML → parse → extract features → index to Meilisearch

### Key Technologies

- **Message Queue:** Redpanda (Kafka-compatible, via rdkafka crate)
- **Search:** Meilisearch (primary store for documents, metadata, vectors)
- **Local State:** RocksDB (per-worker URL cache, robots.txt, DNS)
- **Cache:** DragonflyDB/Redis (rate limiting, real-time counters)
- **Object Storage:** S3-compatible (RustFS/MinIO) for HTML archives

### Near-Duplicate Detection

The frontier uses dual locality-sensitive hashing:
- **SimHash:** 64-bit fingerprints for quick similarity checks (Hamming distance threshold ~10 bits)
- **MinHash:** 128 hash functions for accurate Jaccard similarity estimation (threshold ~0.8)

## Marketing Product Pages

Product landing pages live in `console/src/app/(marketing)/products/{scrape,map,crawl,search}/page.tsx`. All pages follow a consistent section structure and visual pattern.

### Section Order

1. **Hero** — Badge with icon + endpoint name, h1 with glitch-font highlighted word, subtitle, CTA buttons (Try it free / See pricing)
2. **Code example** — Faux-terminal with three dot header, curl command, and inline JSON response preview
3. **How it works** — Numbered steps (`"01"`, `"02"`, etc.) using Rubik Glitch font (`var(--font-rubik-glitch)`) at `text-3xl` with gradient text
4. **Features grid** — 6 cards in a 3-col grid, each with a 10x10 icon container (`rounded-xl bg-gradient-to-br ... ring-1 ring-white/10`)
5. **Checklist** — Two-column grid of features with green checkmark icons
6. **Pricing summary** — Simple label/value rows in a bordered card, link to full pricing
7. **CTA** — Centered heading + subtitle + single button, background glow

### Color Themes Per Product

| Product | Primary | Gradient | Glow |
|---------|---------|----------|------|
| Scrape | `indigo-400` | `from-indigo-400 to-cyan-400` | `bg-indigo-500/10` |
| Map | `cyan-400` | `from-cyan-400 to-indigo-400` | `bg-cyan-500/10` |
| Crawl | `violet-400` | `from-violet-400 to-indigo-400` | `bg-violet-500/10` |
| Search | `emerald-400` | `from-emerald-400 to-cyan-400` | `bg-emerald-500/10` |

### Key Conventions

- All API URLs in examples use `https://scrapix.meilisearch.dev`
- Hero highlighted word uses `style={{ fontFamily: "var(--font-rubik-glitch), var(--font-geist-sans), sans-serif" }}`
- Step numbers use the same glitch font with gradient `bg-clip-text text-transparent`
- Terminal response uses color classes: `text-indigo-400` for keys, `text-emerald-400` for strings, `text-cyan-400` for numbers, `text-zinc-600` for punctuation
- Navigation links exist in both the header dropdown and footer in `console/src/app/(marketing)/layout.tsx`

## Billing Data Model

The system tracks usage data for pricing/billing purposes.

### Data Tracked Per Request

| Field | Type | Description |
|-------|------|-------------|
| `account_id` | String | Account for billing attribution |
| `content_length` | u64 | Bytes downloaded (bandwidth billing) |
| `js_rendered` | bool | Premium JS rendering feature |
| `job_id` | String | Job attribution |
| `domain` | String | Domain crawled |

### Billing Types (scrapix-core)

- `Account` - Billable entity with tier and quotas
- `ApiKey` - Authentication token linked to account
- `BillingTier` - Free/Starter/Pro/Enterprise with limits
- `UsageMetrics` - Per-period usage tracking

### ClickHouse Analytics Queries

```sql
-- Account usage summary
SELECT account_id, count() as pages, sum(content_length) as bytes
FROM crawl_events
WHERE crawled_at >= now() - INTERVAL 30 DAY
GROUP BY account_id;

-- Daily breakdown for billing
SELECT toDate(crawled_at) as date, count() as requests, sum(content_length) as bytes
FROM crawl_events
WHERE account_id = 'acct_123'
GROUP BY date ORDER BY date;
```

### API Endpoints for Billing

- `GET /analytics/v0/pipes/account_usage.json?account_id=X&hours=24` - Account usage
- `GET /analytics/v0/pipes/top_accounts.json?hours=24&limit=10` - Top accounts

## Environment Variables

| Variable | Description |
|----------|-------------|
| `KAFKA_BROKERS` | Kafka/Redpanda broker addresses |
| `MEILISEARCH_URL` | Meilisearch server URL |
| `MEILISEARCH_API_KEY` | Meilisearch API key |
| `REDIS_URL` | Redis/DragonflyDB URL |
| `CLICKHOUSE_URL` | ClickHouse HTTP URL (enables analytics API) |
| `CLICKHOUSE_DATABASE` | ClickHouse database name (default: scrapix) |
| `CLICKHOUSE_USER` | ClickHouse username |
| `CLICKHOUSE_PASSWORD` | ClickHouse password |
| `RUST_LOG` | Log level (info, debug, trace) |
| `OPENAI_API_KEY` | For AI enrichment features |

## Kubernetes Deployment

```bash
# Local development (Docker Desktop)
scrapix k8s deploy
scrapix k8s port-forward

# Production
scrapix k8s deploy -o prod

# Or manually with kubectl:
kubectl apply -k deploy/kubernetes/overlays/local
kubectl port-forward -n scrapix svc/scrapix-api 8080:8080
```

## CLI Usage Guide (for Testing, Benchmarking, and Review)

This section is a guide for using the Scrapix CLI to test, benchmark, and review crawling operations.

### Quick Reference

| Task | Command |
|------|---------|
| Start infrastructure | `scrapix infra up` |
| Stop infrastructure | `scrapix infra down` |
| Run distributed crawl | `scrapix crawl -p config.json` |
| Run standalone crawl | `scrapix local -p config.json` |
| Check system status | `scrapix stats` |
| View errors | `scrapix errors --last 20` |
| View domain stats | `scrapix domains --top 10` |
| Run benchmarks | `scrapix bench all` |
| Deploy to Kubernetes | `scrapix k8s deploy` |
| Show K8s status | `scrapix k8s status` |

### Infrastructure Commands

```bash
# Start infrastructure (Redpanda, Meilisearch, DragonflyDB)
scrapix infra up

# Stop infrastructure
scrapix infra down

# Restart infrastructure
scrapix infra restart

# Show status
scrapix infra status

# View logs (optionally for specific service)
scrapix infra logs
scrapix infra logs redpanda -f

# Full reset (removes all data volumes)
scrapix infra reset
scrapix infra reset -y  # Skip confirmation
```

### Test Workflows

#### 1. Quick Single-Page Test (No Infrastructure)

For testing parser/extractor changes without starting Kafka/Meilisearch:

```bash
# Standalone crawl - fetches, parses, outputs result directly
scrapix local -c '{"start_urls":["https://example.com"],"index_uid":"test"}'
scrapix local -p config.json --output results.json
```

This bypasses the distributed system entirely. Useful for:
- Testing HTML parsing changes
- Debugging content extraction
- Quick validation without infrastructure overhead

#### 2. Full Distributed Test

For testing the complete pipeline (API → Kafka → Workers → Meilisearch):

```bash
# 1. Start infrastructure
scrapix infra up

# 2. Start all services (in separate terminals, or use screen/tmux)
KAFKA_BROKERS=localhost:19092 MEILISEARCH_URL=http://localhost:7700 MEILISEARCH_API_KEY=masterKey cargo run --release --bin scrapix-api &
KAFKA_BROKERS=localhost:19092 cargo run --release --bin scrapix-frontier-service &
KAFKA_BROKERS=localhost:19092 cargo run --release --bin scrapix-worker-crawler &
KAFKA_BROKERS=localhost:19092 MEILISEARCH_URL=http://localhost:7700 MEILISEARCH_API_KEY=masterKey cargo run --release --bin scrapix-worker-content &

# 3. Submit a crawl job
scrapix crawl -p examples/simple-crawl.json

# 4. Monitor progress
scrapix status <job_id>
scrapix stats
scrapix errors --last 10
scrapix domains --top 5
```

#### 3. Crawl Configuration Examples

**Simple single-site crawl:**
```json
{
  "start_urls": ["https://docs.example.com"],
  "max_depth": 3,
  "max_pages": 100,
  "index_uid": "test-crawl"
}
```

**Multi-site crawl with domain restrictions:**
```json
{
  "start_urls": ["https://site1.com", "https://site2.com"],
  "max_depth": 2,
  "max_pages": 500,
  "allowed_domains": ["site1.com", "site2.com"],
  "index_uid": "multi-site-test"
}
```

### Reviewing Crawl Results

#### Check Indexed Documents in Meilisearch

```bash
# Search indexed documents
curl "http://localhost:7700/indexes/test-crawl/search" \
  -H "Authorization: Bearer masterKey" \
  -H "Content-Type: application/json" \
  -d '{"q": "search term", "limit": 10}'

# Get document count
curl "http://localhost:7700/indexes/test-crawl/stats" \
  -H "Authorization: Bearer masterKey"

# Get specific document by ID
curl "http://localhost:7700/indexes/test-crawl/documents/doc_id" \
  -H "Authorization: Bearer masterKey"
```

#### Check Analytics (if ClickHouse enabled)

```bash
# Key metrics
scrapix analytics kpis --hours 24

# Domain performance
scrapix analytics top-domains --limit 10

# Error analysis
scrapix analytics error-dist --hours 24

# Job-specific stats
scrapix analytics job-stats --job-id <job_id>
```

### Benchmarking

```bash
# Run all benchmarks
scrapix bench all

# Run Wikipedia E2E benchmark
scrapix bench wikipedia

# Run integrated component benchmarks
scrapix bench integrated

# Run parser benchmarks
scrapix bench parser

# Run with multiple iterations and verbose output
scrapix bench all -i 3 -v

# Save results to custom directory
scrapix bench wikipedia -o ./my-bench-results
```

**Key benchmark targets:**
- `all` - Both wikipedia_e2e and integrated_benchmarks
- `wikipedia` - Real-world Wikipedia crawling
- `integrated` - Full pipeline performance
- `parser` - Parser/extractor microbenchmarks

### Kubernetes Commands

```bash
# Deploy to Kubernetes (local overlay)
scrapix k8s deploy

# Deploy to production
scrapix k8s deploy -o prod

# Show deployment status
scrapix k8s status
scrapix k8s status -w  # Watch mode

# View logs
scrapix k8s logs           # All components
scrapix k8s logs crawler   # Specific component
scrapix k8s logs -f        # Follow logs

# Scale components
scrapix k8s scale crawler -r 5

# Restart components
scrapix k8s restart        # All
scrapix k8s restart api    # Specific

# Port forward for local access
scrapix k8s port-forward

# Destroy deployment
scrapix k8s destroy
scrapix k8s destroy -y  # Skip confirmation
```

### Troubleshooting

| Issue | Check |
|-------|-------|
| Crawl not progressing | `scrapix stats` - check queue sizes, error counts |
| High error rate | `scrapix errors --last 50` - identify patterns |
| Slow domain | `scrapix domains --filter domain.com` - check avg latency |
| Service not connecting | Check env vars (KAFKA_BROKERS, MEILISEARCH_URL) |
| Kafka issues | `scrapix infra logs redpanda` |

### Clean Up

```bash
# Stop infrastructure
scrapix infra down

# Full reset (removes all data volumes)
scrapix infra reset

# Clean local data directories
rm -rf ./data ./bench-results ./crawl-results
```
