# Webhook

Webhook notification system for real-time crawler event delivery to external services.

## Overview

The webhook module provides reliable delivery of crawler events to configured endpoints with retry logic, signature verification, and batching capabilities.

## Architecture

```
webhook/
└── webhook-manager.ts  # Webhook delivery and management
```

## Webhook Manager

Core webhook functionality:

```typescript
import { WebhookManager } from './webhook-manager';

const webhookManager = new WebhookManager({
  url: 'https://your-webhook.com/endpoint',
  token: 'secret-token',
  maxRetries: 3,
  timeout: 10000,
  batchSize: 10,
  flushInterval: 5000
});

// Send single event
await webhookManager.send({
  event: 'crawl.completed',
  data: {
    urls_crawled: 100,
    documents_sent: 95,
    duration: 120000
  }
});

// Events are automatically batched
```

## Event Types

Standard webhook events:

```typescript
interface WebhookEvents {
  // Crawl lifecycle
  'crawl.started': {
    job_id: string
    config: CrawlerConfig
    timestamp: Date
  }
  
  'crawl.progress': {
    job_id: string
    urls_crawled: number
    documents_sent: number
    current_url?: string
  }
  
  'crawl.completed': {
    job_id: string
    stats: CrawlerStatistics
    duration: number
  }
  
  'crawl.failed': {
    job_id: string
    error: string
    partial_results?: any
  }
  
  // Document events
  'document.processed': {
    url: string
    title: string
    word_count: number
    extracted_data?: any
  }
  
  'document.failed': {
    url: string
    error: string
    retry_count: number
  }
  
  // System events
  'error.critical': {
    message: string
    stack?: string
    context: any
  }
  
  'quota.exceeded': {
    resource: string
    limit: number
    current: number
  }
}
```

## Configuration

### Basic Configuration
```typescript
{
  url: 'https://webhook.site/unique-id',
  token: 'your-secret-token',
  headers: {
    'X-Custom-Header': 'value'
  }
}
```

### Advanced Configuration
```typescript
{
  url: 'https://api.example.com/webhook',
  token: 'secret',
  
  // Retry configuration
  maxRetries: 5,
  retryDelay: 1000,
  retryBackoff: 'exponential',
  
  // Batching
  batchSize: 50,
  flushInterval: 10000,
  maxBatchWait: 30000,
  
  // Security
  signatureHeader: 'X-Webhook-Signature',
  signatureAlgorithm: 'sha256',
  
  // Filtering
  events: ['crawl.completed', 'error.critical'],
  
  // Timeout
  timeout: 30000,
  
  // Compression
  compress: true
}
```

## Webhook Payload

Standard payload structure:

```json
{
  "webhook_id": "wh_123456",
  "timestamp": "2024-01-01T00:00:00.000Z",
  "event": "crawl.completed",
  "data": {
    "job_id": "job_789",
    "stats": {
      "urls_crawled": 100,
      "documents_sent": 95
    }
  },
  "metadata": {
    "app_version": "1.0.0",
    "environment": "production"
  }
}
```

Batch payload:

```json
{
  "webhook_id": "wh_batch_123",
  "timestamp": "2024-01-01T00:00:00.000Z",
  "events": [
    {
      "event": "document.processed",
      "data": { /* ... */ },
      "timestamp": "2024-01-01T00:00:00.000Z"
    },
    {
      "event": "document.processed",
      "data": { /* ... */ },
      "timestamp": "2024-01-01T00:00:01.000Z"
    }
  ],
  "batch_info": {
    "size": 2,
    "first_event": "2024-01-01T00:00:00.000Z",
    "last_event": "2024-01-01T00:00:01.000Z"
  }
}
```

## Security

### Signature Verification

Verify webhook authenticity:

```typescript
import crypto from 'crypto';

function verifyWebhookSignature(
  payload: string,
  signature: string,
  secret: string
): boolean {
  const hash = crypto
    .createHmac('sha256', secret)
    .update(payload)
    .digest('hex');
  
  return crypto.timingSafeEqual(
    Buffer.from(signature),
    Buffer.from(`sha256=${hash}`)
  );
}

// Express middleware
app.post('/webhook', (req, res) => {
  const signature = req.headers['x-webhook-signature'];
  const payload = JSON.stringify(req.body);
  
  if (!verifyWebhookSignature(payload, signature, SECRET)) {
    return res.status(401).send('Invalid signature');
  }
  
  // Process webhook
});
```

### Token Authentication

Bearer token in Authorization header:

```typescript
// Sending side
headers: {
  'Authorization': `Bearer ${token}`
}

// Receiving side
if (req.headers.authorization !== `Bearer ${expectedToken}`) {
  return res.status(401).send('Unauthorized');
}
```

## Retry Logic

Automatic retry with exponential backoff:

```typescript
class WebhookRetry {
  async sendWithRetry(payload: any, options: RetryOptions) {
    let lastError;
    
    for (let attempt = 0; attempt < options.maxRetries; attempt++) {
      try {
        const response = await this.send(payload);
        
        if (response.status >= 200 && response.status < 300) {
          return response;
        }
        
        // 4xx errors - don't retry
        if (response.status >= 400 && response.status < 500) {
          throw new Error(`Client error: ${response.status}`);
        }
        
        // 5xx errors - retry
        lastError = new Error(`Server error: ${response.status}`);
        
      } catch (error) {
        lastError = error;
      }
      
      // Calculate delay
      const delay = Math.min(
        options.initialDelay * Math.pow(2, attempt),
        options.maxDelay
      );
      
      await sleep(delay);
    }
    
    throw lastError;
  }
}
```

## Batching

Efficient event batching:

```typescript
class WebhookBatcher {
  private batch: Event[] = [];
  private timer: NodeJS.Timeout;
  
  add(event: Event) {
    this.batch.push(event);
    
    if (this.batch.length >= this.batchSize) {
      this.flush();
    } else if (!this.timer) {
      this.timer = setTimeout(() => this.flush(), this.flushInterval);
    }
  }
  
  async flush() {
    if (this.batch.length === 0) return;
    
    const events = [...this.batch];
    this.batch = [];
    
    clearTimeout(this.timer);
    this.timer = null;
    
    await this.sendBatch(events);
  }
}
```

## Error Handling

Handle webhook delivery failures:

```typescript
webhookManager.on('error', (error, context) => {
  console.error('Webhook failed:', error);
  console.log('Context:', context);
  
  // Store failed webhooks for retry
  failedWebhooks.push({
    payload: context.payload,
    error: error.message,
    timestamp: new Date()
  });
});

webhookManager.on('retry', (attempt, maxAttempts) => {
  console.log(`Retry ${attempt}/${maxAttempts}`);
});

webhookManager.on('success', (response) => {
  console.log('Webhook delivered:', response.status);
});
```

## Dead Letter Queue

Store failed webhooks for manual retry:

```typescript
class DeadLetterQueue {
  async store(webhook: FailedWebhook) {
    await db.failedWebhooks.create({
      payload: webhook.payload,
      error: webhook.error,
      attempts: webhook.attempts,
      last_attempt: webhook.lastAttempt
    });
  }
  
  async retry(id: string) {
    const webhook = await db.failedWebhooks.findById(id);
    
    try {
      await webhookManager.send(webhook.payload);
      await db.failedWebhooks.delete(id);
    } catch (error) {
      await db.failedWebhooks.update(id, {
        attempts: webhook.attempts + 1,
        last_attempt: new Date(),
        last_error: error.message
      });
    }
  }
}
```

## Testing Webhooks

Test webhook delivery:

```typescript
// Mock webhook server
import express from 'express';

const mockServer = express();
mockServer.use(express.json());

mockServer.post('/webhook', (req, res) => {
  console.log('Received webhook:', req.body);
  
  // Verify signature
  const signature = req.headers['x-webhook-signature'];
  if (!verifySignature(req.body, signature)) {
    return res.status(401).send('Invalid signature');
  }
  
  // Simulate processing
  if (Math.random() > 0.8) {
    // Random failure for testing retry
    return res.status(500).send('Server error');
  }
  
  res.status(200).json({ received: true });
});

mockServer.listen(3001);
```

## Webhook Services Integration

### Webhook.site
```typescript
{
  url: 'https://webhook.site/unique-id',
  // No authentication needed for testing
}
```

### Zapier
```typescript
{
  url: 'https://hooks.zapier.com/hooks/catch/123456/abcdef/',
  headers: {
    'Content-Type': 'application/json'
  }
}
```

### Discord
```typescript
{
  url: 'https://discord.com/api/webhooks/id/token',
  transform: (event) => ({
    content: `Crawl completed: ${event.data.urls_crawled} URLs processed`,
    embeds: [{
      title: 'Crawl Statistics',
      fields: [
        { name: 'URLs', value: event.data.urls_crawled },
        { name: 'Documents', value: event.data.documents_sent }
      ]
    }]
  })
}
```

### Slack
```typescript
{
  url: 'https://hooks.slack.com/services/T00/B00/XXX',
  transform: (event) => ({
    text: `Crawl ${event.event}`,
    attachments: [{
      color: event.event === 'completed' ? 'good' : 'danger',
      fields: Object.entries(event.data).map(([k, v]) => ({
        title: k,
        value: v,
        short: true
      }))
    }]
  })
}
```

## Best Practices

1. **Always verify signatures** - Ensure webhook authenticity
2. **Implement idempotency** - Handle duplicate deliveries
3. **Set reasonable timeouts** - Avoid hanging connections
4. **Use exponential backoff** - Be respectful with retries
5. **Batch when possible** - Reduce HTTP overhead
6. **Monitor delivery** - Track success/failure rates
7. **Implement circuit breaker** - Stop trying failing endpoints
8. **Store failed webhooks** - Allow manual retry
9. **Document your webhooks** - Provide clear integration guide
10. **Version your events** - Allow backward compatibility