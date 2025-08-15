# Crawlers

Web crawler implementations using different browser engines and parsing strategies.

## Overview

This module provides a factory-based system for creating crawlers with different capabilities. Each crawler type is optimized for specific use cases.

## Architecture

```
crawlers/
├── factory.ts      # Crawler factory for instantiation
├── base.ts         # Abstract base crawler with shared logic
├── cheerio.ts      # Fast HTML-only crawler
├── puppeteer.ts    # Chrome-based crawler with JS support
├── playwright.ts   # Cross-browser crawler
└── index.ts        # Public exports
```

## Crawler Types

### CheerioCrawler
- **Speed**: ⚡⚡⚡⚡⚡ (Fastest)
- **JavaScript Support**: ❌
- **Use Cases**: Static sites, documentation, blogs
- **Memory Usage**: Low
- **Dependencies**: cheerio

```typescript
// Best for static HTML sites
const crawler = new CheerioCrawler(config);
```

### PuppeteerCrawler
- **Speed**: ⚡⚡⚡
- **JavaScript Support**: ✅
- **Use Cases**: SPAs, dynamic content, screenshots
- **Memory Usage**: High
- **Dependencies**: puppeteer, Chrome

```typescript
// Best for JavaScript-heavy sites
const crawler = new PuppeteerCrawler(config);
```

### PlaywrightCrawler
- **Speed**: ⚡⚡⚡
- **JavaScript Support**: ✅
- **Use Cases**: Cross-browser testing, complex interactions
- **Memory Usage**: High
- **Dependencies**: playwright, multiple browsers

```typescript
// Best for cross-browser compatibility
const crawler = new PlaywrightCrawler(config);
```

## Base Crawler

All crawlers extend `BaseCrawler` which provides:

### Core Features
- URL queue management
- Request deduplication
- Error handling and retries
- Event emission
- Statistics tracking
- Feature pipeline execution

### Lifecycle Methods
```typescript
abstract class BaseCrawler {
  // Setup crawler instance
  protected abstract setupCrawler(): Promise<void>
  
  // Process single page
  protected abstract processPage(context: any): Promise<void>
  
  // Start crawling
  async run(): Promise<CrawlerStatistics>
  
  // Stop crawling
  async stop(): Promise<void>
}
```

### Event System
```typescript
crawler.on('progress', ({ urls_crawled, documents_sent }) => {
  console.log(`Progress: ${urls_crawled} URLs crawled`);
});

crawler.on('error', ({ error, url }) => {
  console.error(`Error crawling ${url}:`, error);
});

crawler.on('document', ({ document }) => {
  console.log('Document processed:', document.url);
});
```

## Factory Pattern

The factory automatically selects the appropriate crawler:

```typescript
const crawler = await Crawler.create({
  crawler_type: 'puppeteer', // or 'cheerio', 'playwright'
  start_urls: ['https://example.com'],
  // ... other config
});
```

## Configuration

### Common Options
```typescript
interface CrawlerConfig {
  // URLs
  start_urls: string[]
  sitemap_urls?: string[]
  excluded_urls?: string[]
  
  // Limits
  max_pages_to_crawl?: number
  max_concurrency?: number
  max_requests_per_minute?: number
  
  // Timeouts
  request_handler_timeout_secs?: number
  navigation_timeout_secs?: number
  
  // Features
  features?: FeatureConfig
}
```

### Browser-Specific Options
```typescript
interface BrowserConfig {
  headless?: boolean
  browser_type?: 'chromium' | 'firefox' | 'webkit'
  browser_args?: string[]
  wait_for_selector?: string
  wait_for_timeout?: number
  screenshot?: boolean
}
```

## URL Management

### Queue System
- Automatic discovery of new URLs
- Priority queue for important pages
- Deduplication to avoid revisiting
- Retry failed URLs with backoff

### URL Filtering
```typescript
{
  // Include only matching URLs
  include_urls: ['/docs/*', '/api/*'],
  
  // Exclude matching URLs
  excluded_urls: ['/admin/*', '*.pdf'],
  
  // Maximum crawl depth
  max_crawl_depth: 3
}
```

## Performance

### Concurrency Control
```typescript
{
  max_concurrency: 10,        // Parallel pages
  max_requests_per_minute: 60 // Rate limiting
}
```

### Memory Management
- Automatic browser restart on memory threshold
- Page recycling to prevent leaks
- Batch processing for large crawls

### Optimization Tips
1. Use Cheerio for static sites (10x faster)
2. Limit concurrency to avoid blocking
3. Enable request caching for development
4. Use specific selectors to wait for content
5. Disable images/CSS for faster loading

## Error Handling

### Retry Logic
```typescript
{
  max_request_retries: 3,
  retry_delay_ms: 1000,
  exponential_backoff: true
}
```

### Error Types
- **Network errors**: Automatic retry
- **Timeout errors**: Skip and continue
- **Parse errors**: Log and continue
- **Memory errors**: Restart browser

## Extending Crawlers

Create custom crawler by extending BaseCrawler:

```typescript
import { BaseCrawler } from './base';

export class CustomCrawler extends BaseCrawler {
  protected async setupCrawler() {
    // Initialize your crawler
  }
  
  protected async processPage(context: any) {
    // Process each page
    const document = await this.extractContent(context);
    await this.sendDocument(document);
  }
}
```

## Best Practices

1. **Choose the right crawler**: Cheerio for speed, Puppeteer for JS
2. **Respect robots.txt**: Built-in support for robots.txt
3. **Implement delays**: Avoid overwhelming target servers
4. **Handle errors gracefully**: Use event listeners for error handling
5. **Monitor memory**: Set limits for long-running crawls
6. **Test locally first**: Use small `max_pages_to_crawl` for testing