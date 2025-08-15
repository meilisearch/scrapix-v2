# Scrapix

Enterprise-grade web crawling and content extraction platform built as a TypeScript monorepo. Provides intelligent web scraping with AI-powered extraction, designed for integration with Meilisearch search engine.

## Features

- 🚀 **Multiple Crawler Types**: Cheerio (fast), Puppeteer, and Playwright support
- 🤖 **AI-Powered Extraction**: OpenAI integration for intelligent content processing
- 🔍 **Meilisearch Integration**: Direct indexing to Meilisearch search engine
- 📊 **Feature Pipeline**: Modular processing (markdown, metadata, AI summaries, custom selectors)
- 🌐 **Distributed Crawling**: Redis-backed job queue for scalability
- 🔄 **Proxy Support**: Built-in proxy server for enterprise environments
- 📡 **Real-time Updates**: Server-sent events for live crawl progress
- 🎯 **Smart Rate Limiting**: Configurable concurrency and rate limits

## Quick Start

```bash
# Install dependencies
yarn install

# Build all packages
yarn build

# Run the API server
yarn server

# Run the CLI scraper
yarn scrape -p misc/tests/meilisearch/simple.json

# Run in development mode
yarn dev
```

## Project Structure

```
apps/
├── core/      # Core crawling library (Crawlee-based)
├── server/    # REST API with Bull queue
├── cli/       # Command-line interface
└── proxy/     # Proxy server for enterprise proxies
```

## Development

```bash
# Development mode with hot-reload
yarn dev

# Run specific app
cd apps/server && yarn dev
cd apps/core && yarn dev

# Code quality
yarn lint
yarn lint:fix
yarn test
```

## API Server

The API server provides REST endpoints for crawling operations:

```bash
# Start server (default port 8080)
yarn server

# With Redis for job queue
yarn server -r redis://localhost:6379

# Custom port
yarn server -p 3000
```

### Endpoints

- `POST /crawl` - Start async crawl job
- `POST /crawl/sync` - Synchronous crawling
- `GET /job/:id/status` - Check job status
- `GET /job/:id/events` - Server-sent events stream

## CLI Usage

```bash
# Basic crawl with config file
yarn scrape -p config.json

# Inline configuration
yarn scrape -c '{"start_urls":["https://example.com"],"meilisearch_url":"http://localhost:7700","meilisearch_api_key":"masterKey","meilisearch_index_uid":"my_index"}'

# With custom browser
yarn scrape -p config.json -b "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
```

## Configuration

Example configuration file:

```json
{
  "start_urls": ["https://example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "my_index",
  "match_type": "path",
  "max_pages_to_crawl": 50,
  "crawler_type": "cheerio",
  "features": {
    "markdown": { "activated": true },
    "metadata": { "activated": true },
    "ai_extraction": { 
      "activated": true,
      "prompt": "Extract key information..."
    }
  }
}
```

## Deployment

### Fly.io Deployment

```bash
# Deploy main app
fly deploy

# Deploy proxy to multiple regions
yarn deploy:proxy:regions

# Check deployment status
yarn deploy:proxy:status
```

### Docker

```bash
# Build image
docker build -t scrapix .

# Run container
docker run -p 8080:8080 scrapix
```

## Environment Variables

```bash
# AI Features
OPENAI_API_KEY=sk-...

# Production
REDIS_URL=redis://...
WEBHOOK_URL=https://...
WEBHOOK_TOKEN=...

# Proxy (optional)
PROXY_AUTH_ENABLED=true
PROXY_AUTH_USERNAME=user
PROXY_AUTH_PASSWORD=pass
```

## License

MIT