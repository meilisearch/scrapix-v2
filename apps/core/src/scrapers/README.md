# Scrapers

Content extraction and processing features for transforming raw HTML into structured documents.

## Overview

The scrapers module implements a feature pipeline system where each feature processes documents sequentially. Features can be enabled/disabled and configured independently.

## Architecture

```
scrapers/
├── features/           # Individual feature implementations
│   ├── metadata.ts     # Meta tags extraction
│   ├── markdown.ts     # HTML to Markdown conversion
│   ├── full_page.ts    # Complete page content
│   ├── block_split.ts  # Content chunking
│   ├── ai_extraction.ts # AI-powered extraction
│   ├── ai_summary.ts   # AI summarization
│   ├── schema.ts       # Structured data extraction
│   └── custom_selectors.ts # CSS selector extraction
└── index.ts           # Feature pipeline orchestration
```

## Feature Pipeline

Documents flow through enabled features in sequence:

```
HTML → metadata → markdown → block_split → ai_extraction → ai_summary → Document
```

Each feature:
1. Receives the document from previous feature
2. Processes/enriches the document
3. Passes to next feature
4. Returns final document to sender

## Available Features

### metadata
Extracts meta tags and page information.

**Output fields:**
- `title` - Page title
- `description` - Meta description
- `keywords` - Meta keywords
- `author` - Author information
- `og_image` - Open Graph image
- `published_date` - Publication date

**Configuration:**
```typescript
{
  metadata: {
    activated: true,
    include_og: true,  // Open Graph tags
    include_twitter: true  // Twitter Card tags
  }
}
```

### markdown
Converts HTML content to clean Markdown format.

**Features:**
- Preserves formatting (bold, italic, links)
- Maintains heading hierarchy
- Converts lists and tables
- Removes scripts and styles
- Handles code blocks

**Configuration:**
```typescript
{
  markdown: {
    activated: true,
    include_images: true,
    include_links: true,
    code_language: 'auto'  // Auto-detect code language
  }
}
```

### full_page
Indexes complete page content without processing.

**Use cases:**
- Full-text search
- Archival purposes
- Debugging extraction

**Configuration:**
```typescript
{
  full_page: {
    activated: true,
    include_html: false,  // Include raw HTML
    max_size: 1000000    // Maximum characters
  }
}
```

### block_split
Splits content into semantic chunks for better search relevance.

**Features:**
- Semantic splitting by headings/paragraphs
- Configurable chunk size
- Overlap for context preservation
- Maintains document hierarchy

**Configuration:**
```typescript
{
  block_split: {
    activated: true,
    max_size: 1000,      // Maximum chunk size
    overlap: 100,        // Overlap between chunks
    split_by: 'heading', // 'heading', 'paragraph', 'sentence'
    preserve_tables: true
  }
}
```

### ai_extraction
Uses AI to extract structured information from content.

**Capabilities:**
- Custom extraction prompts
- Multiple model support
- JSON output format
- Retry on failure

**Configuration:**
```typescript
{
  ai_extraction: {
    activated: true,
    prompt: "Extract product name, price, and description",
    model: "gpt-4",
    temperature: 0.3,
    max_tokens: 500,
    output_format: "json"
  }
}
```

### ai_summary
Generates concise summaries of page content.

**Features:**
- Configurable summary length
- Multiple summary styles
- Key points extraction
- Language detection

**Configuration:**
```typescript
{
  ai_summary: {
    activated: true,
    max_length: 200,     // Maximum words
    style: 'bullets',    // 'paragraph', 'bullets', 'key_points'
    model: 'gpt-3.5-turbo',
    include_keywords: true
  }
}
```

### schema
Extracts structured data (JSON-LD, microdata, RDFa).

**Supports:**
- Schema.org vocabulary
- Product information
- Article metadata
- Event details
- Organization data

**Configuration:**
```typescript
{
  schema: {
    activated: true,
    types: ['Product', 'Article', 'Event'],
    include_microdata: true,
    include_rdfa: true
  }
}
```

### custom_selectors
Extracts content using CSS selectors.

**Use cases:**
- Site-specific extraction
- Template-based scraping
- Custom field mapping

**Configuration:**
```typescript
{
  custom_selectors: {
    activated: true,
    selectors: {
      title: "h1.article-title",
      author: ".author-name",
      content: "article.main-content",
      sidebar: "aside.sidebar",
      price: ".product-price",
      rating: ".star-rating"
    },
    required: ['title', 'content'],  // Must exist
    multiple: ['tags']  // Can have multiple values
  }
}
```

## Creating Custom Features

Implement the Feature interface:

```typescript
import { Feature, Document, FeatureContext } from '../types';

export class CustomFeature implements Feature {
  name = 'custom_feature';
  
  constructor(private config: any) {}
  
  async execute(
    document: Document,
    context: FeatureContext
  ): Promise<Document> {
    // Access page or Cheerio instance
    const { page, $ } = context;
    
    // Process document
    const processed = {
      ...document,
      custom_field: await this.extract(page || $)
    };
    
    return processed;
  }
  
  private async extract(source: any): Promise<any> {
    // Extraction logic
  }
}
```

## Feature Context

Each feature receives:

```typescript
interface FeatureContext {
  page?: Page           // Puppeteer/Playwright page
  $?: CheerioAPI       // Cheerio instance
  url: string          // Current URL
  config: FeatureConfig // Feature configuration
  crawler_type: string  // Type of crawler
}
```

## Performance Considerations

### Feature Order
1. Lightweight features first (metadata, schema)
2. Heavy processing next (markdown, block_split)
3. AI features last (expensive API calls)

### Optimization Tips
- Disable unused features
- Cache AI responses
- Batch AI requests
- Use streaming for large content
- Implement timeouts for each feature

## Error Handling

Features should handle errors gracefully:

```typescript
async execute(document: Document, context: FeatureContext) {
  try {
    // Process document
    return processedDocument;
  } catch (error) {
    console.error(`Feature ${this.name} failed:`, error);
    // Return original document on failure
    return document;
  }
}
```

## Testing Features

```typescript
import { CustomFeature } from './custom_feature';

describe('CustomFeature', () => {
  it('should extract custom fields', async () => {
    const feature = new CustomFeature({ /* config */ });
    const document = { url: 'test.com', content: 'test' };
    const context = { $: cheerio.load('<html>...</html>') };
    
    const result = await feature.execute(document, context);
    expect(result.custom_field).toBeDefined();
  });
});
```

## Best Practices

1. **Enable only needed features** - Each feature adds processing time
2. **Configure AI features carefully** - They incur API costs
3. **Test selectors thoroughly** - Sites change frequently
4. **Handle missing data** - Not all pages have all fields
5. **Monitor performance** - Track feature execution time
6. **Implement caching** - Cache expensive operations
7. **Use appropriate models** - Balance cost vs quality for AI