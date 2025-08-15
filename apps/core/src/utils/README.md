# Utils

Utility functions and helper modules for common operations across the crawler.

## Overview

This module contains reusable utilities for error handling, HTML processing, HTTP requests, Meilisearch operations, and sitemap parsing.

## Components

```
utils/
├── error_handler.ts      # Centralized error handling
├── html_cleaner.ts       # HTML sanitization and cleaning
├── http_client.ts        # HTTP request wrapper with retry
├── meilisearch_client.ts # Meilisearch client utilities
├── package_version.ts    # Package version detection
└── sitemap.ts           # Sitemap parsing and processing
```

## Error Handler

Centralized error handling with categorization:

```typescript
import { ErrorHandler, ErrorCategory } from './error_handler';

const errorHandler = new ErrorHandler();

// Handle and categorize errors
try {
  await riskyOperation();
} catch (error) {
  const handled = errorHandler.handle(error, {
    context: 'crawling',
    url: 'https://example.com',
    retry: true
  });
  
  if (handled.category === ErrorCategory.NETWORK) {
    // Retry with backoff
  } else if (handled.category === ErrorCategory.FATAL) {
    // Stop crawler
  }
}

// Error categories
enum ErrorCategory {
  NETWORK = 'network',      // Connection issues
  TIMEOUT = 'timeout',      // Operation timeouts
  PARSING = 'parsing',      // HTML/JSON parsing
  VALIDATION = 'validation', // Data validation
  RATE_LIMIT = 'rate_limit', // API limits
  AUTH = 'auth',           // Authentication
  FATAL = 'fatal'          // Unrecoverable
}
```

### Error Recovery Strategies

```typescript
class ErrorRecovery {
  async withRetry<T>(
    operation: () => Promise<T>,
    options: {
      maxRetries: number
      backoff: 'linear' | 'exponential'
      initialDelay: number
    }
  ): Promise<T> {
    let lastError;
    
    for (let i = 0; i < options.maxRetries; i++) {
      try {
        return await operation();
      } catch (error) {
        lastError = error;
        const delay = this.calculateDelay(i, options);
        await sleep(delay);
      }
    }
    
    throw lastError;
  }
}
```

## HTML Cleaner

Sanitize and clean HTML content:

```typescript
import { HTMLCleaner } from './html_cleaner';

const cleaner = new HTMLCleaner();

// Remove scripts and styles
const clean = cleaner.clean(html, {
  removeScripts: true,
  removeStyles: true,
  removeComments: true,
  removeAttributes: ['onclick', 'onload']
});

// Extract text only
const text = cleaner.extractText(html, {
  preserveWhitespace: false,
  maxLength: 10000
});

// Sanitize for storage
const safe = cleaner.sanitize(html, {
  allowedTags: ['p', 'h1', 'h2', 'h3', 'ul', 'li', 'a'],
  allowedAttributes: {
    'a': ['href', 'title']
  }
});

// Fix broken HTML
const fixed = cleaner.repair(html);
```

### Content Extraction

```typescript
// Extract specific content
const extractor = new ContentExtractor();

// Get main content (removes nav, footer, ads)
const main = extractor.getMainContent(html);

// Extract article
const article = extractor.extractArticle(html, {
  minTextLength: 100,
  minScore: 20
});

// Remove boilerplate
const content = extractor.removeBoilerplate(html);
```

## HTTP Client

Robust HTTP client with retry and timeout:

```typescript
import { HTTPClient } from './http_client';

const client = new HTTPClient({
  timeout: 30000,
  retries: 3,
  retryDelay: 1000,
  headers: {
    'User-Agent': 'Scrapix/1.0'
  }
});

// GET request
const response = await client.get('https://api.example.com/data', {
  params: { page: 1, limit: 10 }
});

// POST request
const result = await client.post('https://api.example.com/create', {
  data: { name: 'Test' },
  headers: { 'Authorization': 'Bearer token' }
});

// With retry strategy
const data = await client.request({
  url: 'https://api.example.com',
  method: 'GET',
  retry: {
    retries: 5,
    factor: 2, // Exponential backoff
    minTimeout: 1000,
    maxTimeout: 30000,
    randomize: true
  }
});
```

### Request Interceptors

```typescript
// Add request interceptor
client.interceptors.request.use((config) => {
  config.headers['X-Request-ID'] = generateId();
  return config;
});

// Add response interceptor
client.interceptors.response.use(
  (response) => {
    // Log successful requests
    logger.info(`${response.config.method} ${response.config.url}`);
    return response;
  },
  (error) => {
    // Handle errors globally
    if (error.response?.status === 429) {
      // Rate limited - wait and retry
    }
    return Promise.reject(error);
  }
);
```

## Meilisearch Client

Enhanced Meilisearch client with batching and retries:

```typescript
import { MeilisearchClient } from './meilisearch_client';

const client = new MeilisearchClient({
  host: 'http://localhost:7700',
  apiKey: 'masterKey',
  batchSize: 1000,
  flushInterval: 5000
});

// Add documents with auto-batching
await client.addDocuments('index', documents);

// Update index settings
await client.updateSettings('index', {
  searchableAttributes: ['title', 'content'],
  filterableAttributes: ['category', 'date'],
  sortableAttributes: ['date'],
  rankingRules: [
    'words',
    'typo',
    'proximity',
    'attribute',
    'sort',
    'exactness'
  ]
});

// Search with highlighting
const results = await client.search('index', 'query', {
  limit: 20,
  offset: 0,
  filter: 'category = "docs"',
  facets: ['category'],
  highlightPreTag: '<mark>',
  highlightPostTag: '</mark>'
});

// Wait for task completion
const task = await client.waitForTask(taskId, {
  timeOutMs: 30000,
  intervalMs: 100
});
```

### Index Management

```typescript
// Create index with primary key
await client.createIndex('products', 'id');

// Get index stats
const stats = await client.getStats('products');
console.log(`Documents: ${stats.numberOfDocuments}`);
console.log(`Index size: ${stats.databaseSize}`);

// Delete documents
await client.deleteDocuments('products', {
  filter: 'stock = 0'
});

// Clear index
await client.clearIndex('products');
```

## Package Version

Detect and compare package versions:

```typescript
import { PackageVersion } from './package_version';

// Get current version
const version = PackageVersion.get();
console.log(`Running v${version}`);

// Compare versions
if (PackageVersion.isNewer('2.0.0', '1.5.0')) {
  // Upgrade available
}

// Parse version
const parsed = PackageVersion.parse('1.2.3-beta.1');
// { major: 1, minor: 2, patch: 3, prerelease: 'beta.1' }

// Check compatibility
if (PackageVersion.satisfies('^1.0.0', '1.5.0')) {
  // Compatible
}
```

## Sitemap Parser

Parse and process XML sitemaps:

```typescript
import { SitemapParser } from './sitemap';

const parser = new SitemapParser();

// Parse sitemap
const urls = await parser.parse('https://example.com/sitemap.xml');

// Parse with options
const filtered = await parser.parse(sitemapUrl, {
  filterUrls: (url) => url.includes('/docs/'),
  maxUrls: 1000,
  followSitemapIndex: true
});

// Parse sitemap index
const sitemaps = await parser.parseSitemapIndex(indexUrl);
for (const sitemap of sitemaps) {
  const urls = await parser.parse(sitemap.loc);
  console.log(`${sitemap.loc}: ${urls.length} URLs`);
}

// Get URL metadata
urls.forEach(url => {
  console.log(`URL: ${url.loc}`);
  console.log(`Last modified: ${url.lastmod}`);
  console.log(`Change frequency: ${url.changefreq}`);
  console.log(`Priority: ${url.priority}`);
});
```

### Sitemap Validation

```typescript
// Validate sitemap format
const isValid = await parser.validate(sitemapUrl);

// Check sitemap accessibility
const accessible = await parser.checkAccess(sitemapUrl);

// Get sitemap info
const info = await parser.getInfo(sitemapUrl);
console.log(`Total URLs: ${info.urlCount}`);
console.log(`Compressed: ${info.compressed}`);
console.log(`Size: ${info.sizeBytes} bytes`);
```

## Utility Functions

### Sleep/Delay
```typescript
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

// Usage
await sleep(1000); // Wait 1 second
```

### URL Utilities
```typescript
// Normalize URL
const normalized = normalizeUrl('https://example.com//path/');
// https://example.com/path

// Get domain
const domain = getDomain('https://sub.example.com/path');
// example.com

// Is valid URL
if (isValidUrl('https://example.com')) {
  // Valid
}

// Resolve relative URLs
const absolute = resolveUrl('/path', 'https://example.com');
// https://example.com/path
```

### String Utilities
```typescript
// Truncate with ellipsis
const truncated = truncate('Long text...', 50);

// Slugify
const slug = slugify('Hello World!');
// hello-world

// Remove HTML tags
const text = stripHtml('<p>Hello</p>');
// Hello

// Escape regex
const pattern = escapeRegex('file.txt');
// file\.txt
```

## Best Practices

1. **Use appropriate error handling** - Categorize and handle errors properly
2. **Implement retry logic** - Use exponential backoff for transient failures
3. **Sanitize HTML** - Always clean user-generated content
4. **Batch operations** - Group API calls for efficiency
5. **Cache when possible** - Reduce redundant operations
6. **Validate inputs** - Check data before processing
7. **Use timeouts** - Prevent hanging operations