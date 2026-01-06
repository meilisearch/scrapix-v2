# Scrapix

High-performance, distributed web crawler and search indexer built in Rust.

## Vision

Scrapix aims to be an internet-scale web crawler capable of:

1. **Global Internet Indexing** - Crawl billions of pages
2. **Targeted Site Crawling** - Index specific websites/documentation
3. **Real-time Information Retrieval** - On-demand crawling

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                         API Layer                                │
│                     (scrapix-api)                                │
└─────────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│   Frontier    │   │   Crawler     │   │   Content     │
│   Service     │   │   Workers     │   │   Workers     │
└───────────────┘   └───────────────┘   └───────────────┘
        │                     │                     │
        └─────────────────────┼─────────────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Data Layer                               │
│  Redpanda │ RocksDB │ Meilisearch │ DragonflyDB │ S3            │
└─────────────────────────────────────────────────────────────────┘
```

## Tech Stack

| Component | Technology |
|-----------|------------|
| Language | Rust |
| Message Queue | Redpanda (Kafka-compatible) |
| Search | Meilisearch |
| Local State | RocksDB |
| Cache | DragonflyDB (Redis-compatible) |
| Object Storage | S3/MinIO/RustFS |

## Quick Start

### Prerequisites

- Rust 1.75+
- Docker & Docker Compose

### 1. Start Infrastructure

```bash
# Start all infrastructure (Redpanda, Meilisearch, DragonflyDB)
docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d
```

### 2. Build the Project

```bash
cargo build --release
```

### 3. Run Services

In separate terminals:

```bash
# Terminal 1: API Server
KAFKA_BROKERS=localhost:19092 \
MEILISEARCH_URL=http://localhost:7700 \
MEILISEARCH_API_KEY=masterKey \
cargo run --release --bin scrapix-api

# Terminal 2: Frontier Service
KAFKA_BROKERS=localhost:19092 \
cargo run --release --bin scrapix-frontier-service

# Terminal 3: Crawler Worker
KAFKA_BROKERS=localhost:19092 \
cargo run --release --bin scrapix-worker-crawler

# Terminal 4: Content Worker
KAFKA_BROKERS=localhost:19092 \
MEILISEARCH_URL=http://localhost:7700 \
MEILISEARCH_API_KEY=masterKey \
cargo run --release --bin scrapix-worker-content
```

### 4. Start a Crawl

```bash
# Using the CLI
cargo run --bin scrapix -- crawl -f examples/simple-crawl.json

# Or using curl
curl -X POST http://localhost:8080/crawl \
  -H "Content-Type: application/json" \
  -d @examples/simple-crawl.json
```

## Full Stack with Docker

Run everything in Docker:

```bash
# Build and start all services
docker compose up -d --build

# View logs
docker compose logs -f

# Stop all services
docker compose down
```

Services will be available at:
- API: http://localhost:8080
- Meilisearch: http://localhost:7700
- Redpanda Console: http://localhost:8090

## API Reference

### Start Crawl Job (Async)

```bash
POST /crawl
Content-Type: application/json

{
  "start_urls": ["https://example.com"],
  "index_uid": "my-index",
  "max_depth": 5,
  "features": {
    "markdown": { "enabled": true }
  }
}

# Response
{ "job_id": "550e8400-e29b-41d4-a716-446655440000" }
```

### Start Crawl Job (Sync)

```bash
POST /crawl/sync
Content-Type: application/json

# Same body as above, waits for completion
```

### Get Job Status

```bash
GET /job/{job_id}/status

# Response
{
  "job_id": "...",
  "status": "running",
  "pages_crawled": 150,
  "pages_indexed": 145
}
```

### Stream Job Events (SSE)

```bash
GET /job/{job_id}/events
Accept: text/event-stream

# Returns server-sent events with crawl progress
```

### Cancel Job

```bash
DELETE /job/{job_id}
```

### List Jobs

```bash
GET /jobs?limit=10&offset=0
```

### Health Check

```bash
GET /health
```

## CLI Usage

```bash
# Start a crawl job
scrapix crawl -f config.json
scrapix crawl --start-url https://example.com --index-uid my-index

# Start a sync crawl (wait for completion)
scrapix crawl -f config.json --sync

# Check job status
scrapix status <job_id>
scrapix status <job_id> --watch  # Poll continuously

# Stream job events
scrapix events <job_id>

# List recent jobs
scrapix jobs --limit 20

# Cancel a job
scrapix cancel <job_id>
```

## Configuration

See [examples/](examples/) for configuration examples:

- `simple-crawl.json` - Basic HTTP crawl
- `documentation-site.json` - Documentation with custom selectors
- `ecommerce-products.json` - Product catalog with schema extraction
- `ai-enrichment.json` - AI-powered content enrichment
- `with-proxy.json` - Crawling with proxy rotation

### Configuration Reference

```json
{
  "start_urls": ["https://example.com"],
  "index_uid": "my-index",
  "crawler_type": "http",
  "max_depth": 10,
  "max_pages": 1000,
  "url_patterns": {
    "include": ["https://example.com/**"],
    "exclude": ["**/private/**"],
    "index_only": ["**/docs/**"]
  },
  "sitemap": {
    "enabled": true,
    "urls": ["https://example.com/sitemap.xml"]
  },
  "concurrency": {
    "max_concurrent_requests": 20,
    "browser_pool_size": 5
  },
  "rate_limit": {
    "requests_per_second": 10,
    "per_domain_delay_ms": 100,
    "respect_robots_txt": true
  },
  "proxy": {
    "urls": ["http://proxy:8080"],
    "rotation": "round_robin"
  },
  "features": {
    "metadata": { "enabled": true },
    "markdown": { "enabled": true },
    "block_split": { "enabled": true },
    "schema": {
      "enabled": true,
      "only_types": ["Product", "Article"]
    },
    "custom_selectors": {
      "enabled": true,
      "selectors": {
        "title": "h1",
        "price": ".product-price"
      }
    },
    "ai_extraction": {
      "enabled": true,
      "prompt": "Extract key information...",
      "model": "gpt-4"
    },
    "ai_summary": { "enabled": true },
    "embeddings": {
      "enabled": true,
      "model": "text-embedding-3-small"
    }
  },
  "meilisearch": {
    "url": "http://localhost:7700",
    "api_key": "masterKey",
    "batch_size": 100
  },
  "webhooks": [{
    "url": "https://hooks.example.com/crawl",
    "events": ["crawl_completed"],
    "enabled": true
  }]
}
```

## Kubernetes Deployment

### Local (Docker Desktop)

```bash
# Apply local overlay
kubectl apply -k deploy/kubernetes/overlays/local

# Access services
kubectl port-forward -n scrapix svc/scrapix-api 8080:8080
kubectl port-forward -n scrapix svc/meilisearch 7700:7700
```

### Production

```bash
# Apply production overlay
kubectl apply -k deploy/kubernetes/overlays/prod
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `HOST` | API server host | `0.0.0.0` |
| `PORT` | API server port | `8080` |
| `KAFKA_BROKERS` | Kafka/Redpanda brokers | `localhost:9092` |
| `KAFKA_GROUP_ID` | Consumer group ID | Service-specific |
| `MEILISEARCH_URL` | Meilisearch URL | `http://localhost:7700` |
| `MEILISEARCH_API_KEY` | Meilisearch API key | - |
| `REDIS_URL` | Redis/DragonflyDB URL | `redis://localhost:6379` |
| `RUST_LOG` | Log level | `info` |
| `CONCURRENCY` | Crawler concurrency | `10` |
| `USER_AGENT` | HTTP User-Agent | Scrapix default |
| `REQUEST_TIMEOUT` | Request timeout (seconds) | `30` |
| `MAX_RETRIES` | Max retry attempts | `3` |
| `MAX_DEPTH` | Max crawl depth | `100` |
| `RESPECT_ROBOTS` | Respect robots.txt | `true` |
| `OPENAI_API_KEY` | OpenAI API key (for AI features) | - |

## Project Structure

```
scrapix/
├── crates/                    # Library crates
│   ├── scrapix-core/          # Core types and traits
│   ├── scrapix-frontier/      # URL frontier with dedup
│   ├── scrapix-crawler/       # HTTP/browser fetching
│   ├── scrapix-parser/        # HTML parsing
│   ├── scrapix-extractor/     # Feature extraction
│   ├── scrapix-ai/            # AI enrichment
│   ├── scrapix-storage/       # Storage backends
│   ├── scrapix-queue/         # Message queue
│   └── scrapix-telemetry/     # Observability
│
├── bins/                      # Binary crates
│   ├── scrapix-api/           # REST API server
│   ├── scrapix-worker-crawler/# Crawler worker
│   ├── scrapix-worker-content/# Content processor
│   ├── scrapix-frontier-service/# Frontier service
│   └── scrapix-cli/           # CLI tool
│
├── deploy/                    # Deployment configs
│   └── kubernetes/            # K8s manifests
│       ├── base/              # Base resources
│       └── overlays/          # Environment overrides
│           ├── local/         # Local development
│           └── prod/          # Production
│
├── examples/                  # Example configurations
├── ARCHITECTURE.md            # Detailed architecture docs
└── docker-compose.yml         # Docker Compose stack
```

## Documentation

- [Architecture](ARCHITECTURE.md) - System design and tech decisions

## Contributing

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

MIT
