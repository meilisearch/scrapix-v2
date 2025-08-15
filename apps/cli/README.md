# @scrapix/cli

Command-line interface for Scrapix web crawler with direct Meilisearch integration.

## Overview

The CLI tool provides a simple way to crawl websites and index content directly into Meilisearch from the command line. It supports various crawler types and content extraction features.

## Installation

```bash
# From project root
yarn build
yarn scrape --help
```

## Usage

### Basic Usage

```bash
# Crawl with inline configuration
yarn scrape -c '{"start_urls":["https://example.com"],"meilisearch_url":"http://localhost:7700","meilisearch_api_key":"masterKey","meilisearch_index_uid":"my_index"}'

# Crawl with configuration file
yarn scrape -p config.json

# Crawl with custom browser
yarn scrape -p config.json -b "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
```

## Command-Line Options

| Option | Alias | Description | Required |
|--------|-------|-------------|----------|
| `--config` | `-c` | Inline JSON configuration string | One required |
| `--path` | `-p` | Path to JSON configuration file | One required |
| `--browser` | `-b` | Custom browser executable path | No |
| `--verbose` | `-v` | Enable verbose logging | No |
| `--help` | `-h` | Show help message | No |

## Configuration

### Configuration Structure

```json
{
  "start_urls": ["https://example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "my_index",
  "crawler_type": "puppeteer",
  "max_pages_to_crawl": 100,
  "max_concurrency": 10,
  "features": {
    "metadata": {
      "activated": true
    },
    "markdown": {
      "activated": true
    },
    "ai_extraction": {
      "activated": false,
      "prompt": "Extract key information"
    }
  },
  "excluded_urls": ["*/admin/*", "*/login/*"],
  "sitemap_urls": ["https://example.com/sitemap.xml"]
}
```

### Crawler Types

| Type | Description | Use Case |
|------|-------------|----------|
| `cheerio` | Fast HTML parser (no JavaScript) | Static websites, documentation |
| `puppeteer` | Chrome automation (JavaScript support) | SPAs, dynamic content |
| `playwright` | Cross-browser automation | Complex interactions, testing |

### Features

| Feature | Description | Configuration |
|---------|-------------|---------------|
| `metadata` | Extract meta tags and page info | `{"activated": true}` |
| `markdown` | Convert HTML to Markdown | `{"activated": true}` |
| `full_page` | Index entire page content | `{"activated": true}` |
| `block_split` | Split content into semantic blocks | `{"activated": true, "max_size": 1000}` |
| `ai_extraction` | AI-powered content extraction | `{"activated": true, "prompt": "..."}` |
| `ai_summary` | Generate AI summaries | `{"activated": true, "max_length": 200}` |
| `schema` | Extract structured data | `{"activated": true}` |
| `custom_selectors` | Extract specific elements | `{"activated": true, "selectors": {...}}` |

## Examples

### 1. Basic Documentation Crawl
```bash
yarn scrape -c '{
  "start_urls": ["https://docs.example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "docs",
  "crawler_type": "cheerio",
  "features": {
    "metadata": {"activated": true},
    "markdown": {"activated": true}
  }
}'
```

### 2. E-commerce Site with AI Extraction
```bash
yarn scrape -p configs/ecommerce.json
```

`configs/ecommerce.json`:
```json
{
  "start_urls": ["https://shop.example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "products",
  "crawler_type": "puppeteer",
  "features": {
    "ai_extraction": {
      "activated": true,
      "prompt": "Extract product name, price, description, and availability"
    },
    "schema": {
      "activated": true
    }
  },
  "excluded_urls": ["*/cart/*", "*/checkout/*"]
}
```

### 3. Blog with Content Splitting
```bash
yarn scrape -p configs/blog.json
```

`configs/blog.json`:
```json
{
  "start_urls": ["https://blog.example.com"],
  "sitemap_urls": ["https://blog.example.com/sitemap.xml"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "blog",
  "crawler_type": "cheerio",
  "features": {
    "metadata": {"activated": true},
    "markdown": {"activated": true},
    "block_split": {
      "activated": true,
      "max_size": 500
    },
    "ai_summary": {
      "activated": true,
      "max_length": 150
    }
  }
}
```

### 4. SPA with Custom Selectors
```bash
yarn scrape -p configs/spa.json
```

`configs/spa.json`:
```json
{
  "start_urls": ["https://app.example.com"],
  "meilisearch_url": "http://localhost:7700",
  "meilisearch_api_key": "masterKey",
  "meilisearch_index_uid": "app",
  "crawler_type": "playwright",
  "wait_for_selector": ".content-loaded",
  "features": {
    "custom_selectors": {
      "activated": true,
      "selectors": {
        "title": "h1.page-title",
        "content": "main.content",
        "sidebar": "aside.sidebar",
        "breadcrumb": "nav.breadcrumb"
      }
    }
  }
}
```

## Environment Variables

```bash
# OpenAI API Key (for AI features)
OPENAI_API_KEY=sk-...

# Crawlee Storage Directory
CRAWLEE_STORAGE_DIR=./storage

# Custom Browser Path (alternative to -b flag)
PUPPETEER_EXECUTABLE_PATH=/usr/bin/google-chrome

# Proxy Configuration
HTTP_PROXY=http://proxy:8080
HTTPS_PROXY=http://proxy:8080
```

## Output

The CLI will display progress in real-time:

```
🚀 Starting crawl...
📋 Configuration loaded
🎯 Target: https://example.com
📦 Index: my_index

[1/100] ✓ https://example.com/
[2/100] ✓ https://example.com/about
[3/100] ⚠ https://example.com/broken (404)
...

✅ Crawl completed!
📊 Statistics:
  - URLs crawled: 100
  - Documents indexed: 95
  - Errors: 5
  - Duration: 2m 30s
```

## Storage

Crawlee stores temporary data in:
```
./storage/
  ├── request_queues/  # URL queue
  ├── key_value_stores/ # Crawler state
  └── datasets/        # Extracted data
```

Clear storage between runs:
```bash
rm -rf ./storage
```

## Error Handling

The CLI handles various error scenarios:

- **Network errors**: Automatic retry with exponential backoff
- **Rate limiting**: Respects robots.txt and implements delays
- **Memory management**: Automatic garbage collection for large crawls
- **Partial failures**: Continues crawling even if some URLs fail

## Tips

1. **Start small**: Test with `max_pages_to_crawl: 10` first
2. **Use sitemaps**: Faster discovery of all pages
3. **Exclude patterns**: Skip unnecessary pages (login, admin, etc.)
4. **Adjust concurrency**: Lower for respectful crawling, higher for speed
5. **Choose right crawler**: Cheerio for speed, Puppeteer for JavaScript
6. **Monitor memory**: Use `--max-old-space-size=4096` for large crawls

## Debugging

Enable verbose logging:
```bash
yarn scrape -p config.json -v

# Or with environment variable
DEBUG=* yarn scrape -p config.json
```

Check Crawlee storage for details:
```bash
cat storage/key_value_stores/default/SDK_CRAWLER_STATISTICS_0.json
```