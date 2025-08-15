# @scrapix/server

REST API server for Scrapix web crawling platform with job queue management.

## Overview

The server provides a REST API for managing web crawling jobs with Redis-backed queue processing. It supports both synchronous and asynchronous crawling operations with real-time event streaming.

## Features

- 🚀 Asynchronous job processing with Bull queue
- 📊 Real-time crawl progress via Server-Sent Events (SSE)
- 🔄 Synchronous and asynchronous crawling modes
- 📈 OpenTelemetry instrumentation for monitoring
- 🔒 Rate limiting and security middleware
- 🏥 Health checks and status monitoring

## API Endpoints

### Health & Status

#### `GET /health`
Health check endpoint for monitoring.

**Response:**
```json
{
  "status": "ok",
  "timestamp": "2024-01-01T00:00:00.000Z",
  "uptime": 3600,
  "redis": "connected",
  "version": "0.1.9"
}
```

### Crawling

#### `POST /crawl`
Start an asynchronous crawl job.

**Request Body:**
```json
{
  "start_urls": ["https://example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "my_index",
  "max_pages_to_crawl": 100,
  "crawler_type": "puppeteer",
  "features": {
    "metadata": { "activated": true },
    "markdown": { "activated": true }
  }
}
```

**Response:**
```json
{
  "jobId": "job_123456",
  "status": "queued",
  "message": "Crawl job queued successfully"
}
```

#### `POST /crawl/sync`
Start a synchronous crawl (waits for completion).

**Request:** Same as `/crawl`

**Response:**
```json
{
  "status": "completed",
  "urls_crawled": 42,
  "documents_sent": 40,
  "duration": 120000,
  "errors": []
}
```

### Job Management

#### `GET /job/:id/status`
Get the status of a crawl job.

**Response:**
```json
{
  "jobId": "job_123456",
  "status": "processing",
  "progress": {
    "urls_crawled": 25,
    "total_urls": 100,
    "documents_sent": 24
  },
  "startedAt": "2024-01-01T00:00:00.000Z"
}
```

#### `GET /job/:id/events`
Stream real-time events for a crawl job (Server-Sent Events).

**Event Stream:**
```
event: progress
data: {"urls_crawled": 10, "documents_sent": 9}

event: error
data: {"error": "Failed to crawl URL", "url": "https://example.com/page"}

event: complete
data: {"status": "completed", "duration": 120000}
```

### Configuration Management

#### `GET /configs`
List all saved crawler configurations.

**Response:**
```json
[
  {
    "id": "config_123",
    "name": "Production Crawler",
    "created_at": "2024-01-01T00:00:00.000Z",
    "config": { ... }
  }
]
```

#### `POST /configs`
Save a new crawler configuration.

**Request Body:**
```json
{
  "name": "My Config",
  "config": {
    "start_urls": ["https://example.com"],
    "crawler_type": "puppeteer",
    ...
  }
}
```

#### `GET /configs/:id`
Get a specific configuration by ID.

#### `PUT /configs/:id`
Update an existing configuration.

#### `DELETE /configs/:id`
Delete a configuration.

### Run History

#### `GET /runs`
List all crawler run history.

**Query Parameters:**
- `limit` - Number of results (default: 50)
- `offset` - Pagination offset
- `status` - Filter by status (completed, failed, processing)

**Response:**
```json
{
  "runs": [
    {
      "id": "run_123",
      "config_id": "config_456",
      "status": "completed",
      "started_at": "2024-01-01T00:00:00.000Z",
      "completed_at": "2024-01-01T00:02:00.000Z",
      "stats": {
        "urls_crawled": 100,
        "documents_sent": 95,
        "errors": 5
      }
    }
  ],
  "total": 250,
  "limit": 50,
  "offset": 0
}
```

#### `GET /runs/:id`
Get detailed information about a specific run.

#### `GET /runs/:id/events`
Get all events from a specific run.

## Environment Variables

```bash
# Server Configuration
PORT=8080                     # Server port
NODE_ENV=production          # Environment (development/production)

# Redis Configuration (optional)
REDIS_URL=redis://localhost:6379  # Redis connection URL

# OpenTelemetry (optional)
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318  # OTLP endpoint
OTEL_SERVICE_NAME=scrapix-server                   # Service name

# Webhooks (optional)
WEBHOOK_URL=https://your-webhook.com  # Webhook endpoint
WEBHOOK_TOKEN=secret_token           # Webhook authentication

# Supabase (optional - for persistent storage)
SUPABASE_URL=https://xxx.supabase.co
SUPABASE_ANON_KEY=xxx
```

## Running the Server

### Development
```bash
yarn dev        # Run with hot-reload
yarn dev:simple # Build and run once
```

### Production
```bash
yarn build  # Build TypeScript
yarn start  # Start server
```

### Docker
```bash
# Build from project root
docker build -f apps/server/Dockerfile -t scrapix-server .

# Run container
docker run -p 8080:8080 \
  -e REDIS_URL=redis://host.docker.internal:6379 \
  scrapix-server
```

### Deployment to Fly.io
```bash
# From project root
yarn deploy:server
```

## Architecture

The server uses a queue-based architecture:

1. **API Layer** - Express server handling HTTP requests
2. **Queue Layer** - Bull queue for job management (Redis-backed)
3. **Worker Process** - Separate process executing crawl jobs
4. **Event Bus** - Real-time event streaming to clients
5. **Storage Layer** - Optional Supabase for persistent storage

## Rate Limiting

Default rate limits:
- 100 requests per 15 minutes per IP
- Configurable via environment variables

## Error Codes

- `400` - Invalid request parameters
- `404` - Resource not found
- `429` - Rate limit exceeded
- `500` - Internal server error
- `503` - Service unavailable (Redis disconnected)