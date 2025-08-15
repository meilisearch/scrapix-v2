# @scrapix/core

Core crawling library for Scrapix - a powerful web scraping engine built on Crawlee with Meilisearch integration.

## Overview

The core library provides the crawling engine, content extraction features, and Meilisearch integration. It supports multiple crawler types (Cheerio, Puppeteer, Playwright) and a modular feature system for content processing.

## Architecture

```
@scrapix/core/
├── crawlers/       # Crawler implementations
├── scrapers/       # Content extraction features  
├── events/         # Event bus system
├── sender.ts       # Meilisearch document sender
├── container.ts    # Dependency injection
├── supabase/       # Persistent storage
├── telemetry/      # OpenTelemetry integration
├── utils/          # Utility functions
└── webhook/        # Webhook notifications
```

## Core Components

### 1. Crawlers
Factory-based crawler system supporting multiple engines:
- **CheerioCrawler** - Fast HTML parsing without JavaScript
- **PuppeteerCrawler** - Chrome automation with JavaScript support
- **PlaywrightCrawler** - Cross-browser automation

### 2. Scrapers
Modular feature pipeline for content processing:
- **metadata** - Extract meta tags and page information
- **markdown** - Convert HTML to Markdown format
- **full_page** - Index complete page content
- **block_split** - Split content into semantic blocks
- **ai_extraction** - AI-powered content extraction
- **ai_summary** - Generate content summaries
- **schema** - Extract structured data (JSON-LD, microdata)
- **custom_selectors** - Extract specific DOM elements

### 3. Event System
Real-time event bus for crawl progress monitoring:
- Progress updates
- Error reporting
- Document processing events
- Batch completion notifications

### 4. Document Sender
Intelligent batching system for Meilisearch:
- Automatic batching (default: 1000 documents)
- Retry logic with exponential backoff
- Memory-efficient streaming

## Usage

### Basic Example

```typescript
import { Crawler, DocumentBuilder } from '@scrapix/core';

// Create crawler instance
const crawler = await Crawler.create({
  start_urls: ['https://example.com'],
  meilisearch_url: 'http://localhost:7700',
  meilisearch_api_key: 'masterKey',
  meilisearch_index_uid: 'my_index',
  crawler_type: 'puppeteer',
  features: {
    metadata: { activated: true },
    markdown: { activated: true }
  }
});

// Subscribe to events
crawler.on('progress', (data) => {
  console.log(`Crawled ${data.urls_crawled} URLs`);
});

// Start crawling
const stats = await crawler.run();
console.log('Crawl completed:', stats);
```

### Advanced Configuration

```typescript
const config = {
  // URLs
  start_urls: ['https://example.com'],
  sitemap_urls: ['https://example.com/sitemap.xml'],
  
  // Meilisearch
  meilisearch_url: 'http://localhost:7700',
  meilisearch_api_key: 'masterKey',
  meilisearch_index_uid: 'my_index',
  meilisearch_settings: {
    searchableAttributes: ['title', 'content'],
    filterableAttributes: ['category', 'date']
  },
  
  // Crawler settings
  crawler_type: 'puppeteer',
  max_pages_to_crawl: 1000,
  max_concurrency: 10,
  max_requests_per_minute: 60,
  
  // Content extraction
  features: {
    metadata: { activated: true },
    markdown: { activated: true },
    block_split: {
      activated: true,
      max_size: 1000,
      overlap: 100
    },
    ai_extraction: {
      activated: true,
      prompt: 'Extract title, author, date, and main content',
      model: 'gpt-4'
    }
  },
  
  // Filtering
  excluded_urls: ['*/admin/*', '*/private/*'],
  include_urls: ['*/docs/*'],
  
  // Browser options (Puppeteer/Playwright)
  browser_config: {
    headless: true,
    args: ['--no-sandbox']
  },
  wait_for_selector: '.content-loaded',
  
  // Advanced options
  batch_size: 500,
  webhook_url: 'https://your-webhook.com',
  proxy_url: 'http://proxy:8080'
};

const crawler = await Crawler.create(config);
```

## API Reference

### Crawler Factory

```typescript
class Crawler {
  static async create(config: CrawlerConfig): Promise<BaseCrawler>
}
```

### BaseCrawler

```typescript
abstract class BaseCrawler extends EventEmitter {
  async run(): Promise<CrawlerStatistics>
  async stop(): Promise<void>
  on(event: CrawlerEvent, listener: Function): this
}
```

### Events

```typescript
type CrawlerEvents = {
  'progress': { urls_crawled: number, documents_sent: number }
  'document': { document: Document }
  'batch': { batch_size: number, total_sent: number }
  'error': { error: Error, url?: string }
  'complete': { stats: CrawlerStatistics }
}
```

### Document Builder

```typescript
class DocumentBuilder {
  setUrl(url: string): this
  setTitle(title: string): this
  setContent(content: string): this
  setMetadata(metadata: object): this
  addSection(name: string, content: string): this
  build(): Document
}
```

### Features

```typescript
interface Feature {
  name: string
  execute(document: Document, context: FeatureContext): Promise<Document>
}

interface FeatureContext {
  page?: Page  // Puppeteer/Playwright page
  $?: CheerioAPI  // Cheerio instance
  config: FeatureConfig
}
```

## Creating Custom Features

```typescript
import { Feature, Document, FeatureContext } from '@scrapix/core';

class CustomFeature implements Feature {
  name = 'custom_feature';
  
  async execute(
    document: Document, 
    context: FeatureContext
  ): Promise<Document> {
    // Process document
    const processed = {
      ...document,
      custom_field: 'extracted value'
    };
    
    return processed;
  }
}

// Register feature
featureRegistry.register('custom_feature', CustomFeature);
```

## Performance Optimization

### Memory Management
- Documents are processed in batches
- Automatic garbage collection between batches
- Streaming for large responses

### Concurrency Control
```typescript
{
  max_concurrency: 10,  // Parallel requests
  max_requests_per_minute: 60,  // Rate limiting
  max_requests_per_crawl: 10000  // Hard limit
}
```

### Caching
- Request deduplication
- Response caching (configurable TTL)
- State persistence for resume capability

## Error Handling

The library implements robust error handling:

```typescript
crawler.on('error', ({ error, url }) => {
  console.error(`Failed to crawl ${url}:`, error);
});

// Automatic retry with exponential backoff
{
  max_request_retries: 3,
  request_handler_timeout_secs: 30,
  navigation_timeout_secs: 30
}
```

## Testing

```bash
# Run tests
yarn test

# With coverage
yarn test:coverage

# Watch mode
yarn test:watch
```

## Dependencies

- **Crawlee** - Web crawling framework
- **Cheerio** - HTML parsing
- **Puppeteer** - Chrome automation
- **Playwright** - Cross-browser automation
- **Meilisearch** - Search engine client
- **OpenAI** - AI features (optional)
- **Supabase** - Persistent storage (optional)

## Environment Variables

```bash
# Required for AI features
OPENAI_API_KEY=sk-...

# Optional services
SUPABASE_URL=https://xxx.supabase.co
SUPABASE_ANON_KEY=xxx
WEBHOOK_URL=https://webhook.site/xxx
WEBHOOK_TOKEN=secret

# Performance tuning
MAX_CONCURRENCY=10
BATCH_SIZE=1000
```

## License

MIT