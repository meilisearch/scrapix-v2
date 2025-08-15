# Events

Real-time event system for monitoring crawler progress and handling asynchronous communication.

## Overview

The events module provides an event bus for publishing crawler events and subscribing to them. It supports batching, persistence, and real-time streaming.

## Architecture

```
events/
├── crawler-events.ts  # Event type definitions
└── event-bus.ts      # Event bus implementation with batching
```

## Event Types

### CrawlerEvents

```typescript
type CrawlerEvents = {
  // Progress tracking
  'progress': {
    urls_crawled: number
    documents_sent: number
    urls_found: number
    current_url?: string
  }
  
  // Document processing
  'document': {
    document: Document
    url: string
    size: number
  }
  
  // Batch processing
  'batch': {
    batch_size: number
    total_sent: number
    success: boolean
  }
  
  // Errors
  'error': {
    error: Error
    url?: string
    fatal?: boolean
  }
  
  // Completion
  'complete': {
    stats: CrawlerStatistics
    duration: number
  }
}
```

## Event Bus

The `EventBus` class extends Node.js EventEmitter with additional features:

### Features
- Event batching for efficiency
- Auto-flush on size/time thresholds
- Event persistence (optional)
- Metrics collection
- Type-safe event emission

### Basic Usage

```typescript
import { EventBus, getEventBus } from './event-bus';

// Get singleton instance
const eventBus = getEventBus();

// Subscribe to events
eventBus.on('progress', (data) => {
  console.log(`Crawled ${data.urls_crawled} URLs`);
});

eventBus.on('error', ({ error, url }) => {
  console.error(`Error at ${url}:`, error);
});

// Emit events
eventBus.emit('progress', {
  urls_crawled: 10,
  documents_sent: 8,
  urls_found: 25
});
```

## Event Batching

Events are automatically batched for performance:

```typescript
class EventBatch {
  private events: Event[] = []
  private batchSize = 100
  private flushInterval = 5000  // 5 seconds
  
  add(event: Event) {
    this.events.push(event)
    
    if (this.events.length >= this.batchSize) {
      this.flush()
    }
  }
  
  async flush() {
    if (this.events.length === 0) return
    
    // Process batch
    await this.processBatch(this.events)
    this.events = []
  }
}
```

### Configuration

```typescript
const eventBus = new EventBus({
  batchSize: 100,        // Events per batch
  flushInterval: 5000,   // Max time between flushes (ms)
  persistent: true,      // Store events
  maxListeners: 100      // Max event listeners
});
```

## Event Streaming

Stream events to clients via Server-Sent Events (SSE):

```typescript
// Server endpoint
app.get('/events/:jobId', (req, res) => {
  res.writeHead(200, {
    'Content-Type': 'text/event-stream',
    'Cache-Control': 'no-cache',
    'Connection': 'keep-alive'
  });
  
  const listener = (data) => {
    res.write(`event: progress\n`);
    res.write(`data: ${JSON.stringify(data)}\n\n`);
  };
  
  eventBus.on('progress', listener);
  
  req.on('close', () => {
    eventBus.off('progress', listener);
  });
});
```

## Event Persistence

Store events for replay and analysis:

```typescript
import { EventStorage } from '../supabase/event-storage';

class PersistentEventBus extends EventBus {
  private storage = new EventStorage();
  
  async emit(event: string, data: any) {
    // Store event
    await this.storage.store({
      event,
      data,
      timestamp: new Date()
    });
    
    // Emit normally
    super.emit(event, data);
  }
  
  async replay(jobId: string) {
    const events = await this.storage.getEvents(jobId);
    
    for (const event of events) {
      super.emit(event.event, event.data);
    }
  }
}
```

## Metrics Collection

Track event metrics:

```typescript
interface EventMetrics {
  total_events: number
  events_per_second: number
  error_rate: number
  processing_time: number
  batch_efficiency: number
}

class MetricsCollector {
  private metrics: EventMetrics = {
    total_events: 0,
    events_per_second: 0,
    error_rate: 0,
    processing_time: 0,
    batch_efficiency: 0
  };
  
  track(event: string, data: any) {
    this.metrics.total_events++;
    
    if (event === 'error') {
      this.updateErrorRate();
    }
    
    this.calculateEPS();
  }
}
```

## Event Filtering

Filter events by criteria:

```typescript
// Subscribe to specific URLs
eventBus.on('document', (data) => {
  if (data.url.includes('/api/')) {
    // Process API documentation
  }
});

// Error severity filtering
eventBus.on('error', ({ error, fatal }) => {
  if (fatal) {
    // Handle critical errors
    crawler.stop();
  } else {
    // Log warning
    console.warn(error);
  }
});
```

## Event Aggregation

Aggregate events for summary statistics:

```typescript
class EventAggregator {
  private stats = {
    total_urls: 0,
    total_documents: 0,
    total_errors: 0,
    avg_document_size: 0
  };
  
  constructor(eventBus: EventBus) {
    eventBus.on('document', this.onDocument.bind(this));
    eventBus.on('error', this.onError.bind(this));
  }
  
  private onDocument({ size }) {
    this.stats.total_documents++;
    this.updateAverageSize(size);
  }
  
  private onError() {
    this.stats.total_errors++;
  }
  
  getStats() {
    return this.stats;
  }
}
```

## WebSocket Integration

Real-time event broadcasting via WebSocket:

```typescript
import { WebSocketServer } from 'ws';

const wss = new WebSocketServer({ port: 8080 });

// Broadcast events to all connected clients
eventBus.on('progress', (data) => {
  const message = JSON.stringify({
    type: 'progress',
    data
  });
  
  wss.clients.forEach(client => {
    if (client.readyState === WebSocket.OPEN) {
      client.send(message);
    }
  });
});
```

## Error Handling

Robust error handling for event processing:

```typescript
eventBus.on('error', (error) => {
  // Don't let listener errors crash the crawler
});

// Global error handler
eventBus.on('uncaughtException', (error) => {
  console.error('Uncaught exception in event handler:', error);
  // Graceful shutdown
});
```

## Testing Events

```typescript
import { EventBus } from './event-bus';

describe('EventBus', () => {
  it('should batch events', async () => {
    const bus = new EventBus({ batchSize: 2 });
    const spy = jest.fn();
    
    bus.on('batch', spy);
    
    bus.emit('document', { url: 'test1' });
    bus.emit('document', { url: 'test2' });
    
    expect(spy).toHaveBeenCalledWith({
      batch_size: 2,
      success: true
    });
  });
});
```

## Best Practices

1. **Use type-safe events** - Define event types with TypeScript
2. **Handle errors gracefully** - Don't let listener errors crash the app
3. **Batch for performance** - Group events to reduce overhead
4. **Clean up listeners** - Remove listeners when done
5. **Monitor memory** - Too many listeners can cause leaks
6. **Use event namespacing** - Organize events by feature
7. **Document event contracts** - Clear documentation for event data