# Telemetry

OpenTelemetry instrumentation for monitoring, tracing, and metrics collection.

## Overview

This module provides comprehensive observability through OpenTelemetry, including distributed tracing, metrics collection, and performance monitoring.

## Architecture

```
telemetry/
├── index.ts    # Main telemetry setup and configuration
├── openai.ts   # OpenAI API instrumentation
└── queue.ts    # Queue metrics and tracing
```

## Components

### Core Telemetry Setup

Initialize OpenTelemetry with auto-instrumentation:

```typescript
import { setupTelemetry } from './telemetry';

// Initialize telemetry
const telemetry = setupTelemetry({
  serviceName: 'scrapix-crawler',
  serviceVersion: '1.0.0',
  environment: 'production',
  endpoint: 'http://localhost:4318'
});

// Shutdown gracefully
process.on('SIGTERM', async () => {
  await telemetry.shutdown();
});
```

### Tracing

Distributed tracing for request flow:

```typescript
import { trace, SpanStatusCode } from '@opentelemetry/api';

const tracer = trace.getTracer('crawler');

// Create a span
const span = tracer.startSpan('crawl-page');

try {
  // Add attributes
  span.setAttributes({
    'url': 'https://example.com',
    'crawler.type': 'puppeteer',
    'http.method': 'GET'
  });
  
  // Do work
  await crawlPage(url);
  
  // Record success
  span.setStatus({ code: SpanStatusCode.OK });
} catch (error) {
  // Record error
  span.recordException(error);
  span.setStatus({
    code: SpanStatusCode.ERROR,
    message: error.message
  });
} finally {
  span.end();
}
```

### Metrics

Performance and business metrics:

```typescript
import { metrics } from '@opentelemetry/api';

const meter = metrics.getMeter('crawler');

// Counter - total count
const urlCounter = meter.createCounter('urls_crawled', {
  description: 'Total number of URLs crawled'
});

urlCounter.add(1, {
  'crawler.type': 'puppeteer',
  'status': 'success'
});

// Histogram - distribution
const responseTime = meter.createHistogram('response_time', {
  description: 'Response time in milliseconds',
  unit: 'ms'
});

responseTime.record(150, {
  'url': 'https://example.com'
});

// Gauge - current value
const activeConnections = meter.createUpDownCounter('active_connections', {
  description: 'Number of active connections'
});

activeConnections.add(1);  // Connection opened
activeConnections.add(-1); // Connection closed
```

### OpenAI Instrumentation

Track AI API usage and performance:

```typescript
import { instrumentOpenAI } from './openai';

// Wrap OpenAI client
const openai = instrumentOpenAI(new OpenAI({
  apiKey: process.env.OPENAI_API_KEY
}));

// Automatic tracing
const response = await openai.completions.create({
  model: 'gpt-3.5-turbo',
  prompt: 'Extract data...'
});

// Metrics collected:
// - API call count
// - Token usage
// - Response time
// - Error rate
// - Cost estimation
```

### Queue Metrics

Monitor job queue performance:

```typescript
import { QueueMetrics } from './queue';

const queueMetrics = new QueueMetrics(meter);

// Track job processing
queueMetrics.jobStarted('crawl');
queueMetrics.jobCompleted('crawl', 'success', 5000);
queueMetrics.jobFailed('crawl', 'timeout');

// Queue depth
queueMetrics.updateQueueDepth(25);

// Processing rate
queueMetrics.updateProcessingRate(10); // jobs/second
```

## Instrumentation Libraries

Auto-instrumentation for common libraries:

### HTTP/HTTPS
```typescript
// Automatic instrumentation for:
// - axios
// - fetch
// - http/https modules

// Traces include:
// - Request/response headers
// - Status codes
// - Response times
// - Error details
```

### Database
```typescript
// Automatic instrumentation for:
// - PostgreSQL
// - Redis
// - MongoDB

// Traces include:
// - Query text
// - Execution time
// - Row count
// - Connection pool metrics
```

### Express
```typescript
// Automatic instrumentation for Express:
// - Route matching
// - Middleware execution
// - Request/response cycle
// - Error handling
```

## Custom Spans

Create detailed traces for business logic:

```typescript
async function processDocument(url: string) {
  const span = tracer.startSpan('process-document', {
    attributes: {
      'document.url': url,
      'document.type': 'article'
    }
  });
  
  // Create child spans
  await tracer.startActiveSpan('extract-content', async (extractSpan) => {
    const content = await extractContent(url);
    extractSpan.setAttribute('content.length', content.length);
    extractSpan.end();
  });
  
  await tracer.startActiveSpan('ai-processing', async (aiSpan) => {
    const summary = await generateSummary(content);
    aiSpan.setAttribute('summary.length', summary.length);
    aiSpan.end();
  });
  
  span.end();
}
```

## Context Propagation

Maintain trace context across async operations:

```typescript
import { context, propagation } from '@opentelemetry/api';

// Extract context from incoming request
const extractedContext = propagation.extract(
  context.active(),
  request.headers
);

// Run with context
context.with(extractedContext, async () => {
  // All spans created here will be part of the same trace
  await processRequest();
});

// Inject context for outgoing requests
const headers = {};
propagation.inject(context.active(), headers);
```

## Sampling

Control trace sampling for performance:

```typescript
import { TraceIdRatioBasedSampler } from '@opentelemetry/sdk-trace-base';

const sampler = new TraceIdRatioBasedSampler(0.1); // Sample 10%

// Custom sampler
class CustomSampler {
  shouldSample(context, traceId, spanName, spanKind, attributes) {
    // Sample all errors
    if (attributes['error'] === true) {
      return { decision: SamplingDecision.RECORD_AND_SAMPLED };
    }
    
    // Sample 1% of normal traffic
    return Math.random() < 0.01
      ? { decision: SamplingDecision.RECORD_AND_SAMPLED }
      : { decision: SamplingDecision.NOT_RECORD };
  }
}
```

## Exporters

Send telemetry data to backends:

### OTLP (OpenTelemetry Protocol)
```typescript
import { OTLPTraceExporter } from '@opentelemetry/exporter-trace-otlp-http';
import { OTLPMetricExporter } from '@opentelemetry/exporter-metrics-otlp-http';

const traceExporter = new OTLPTraceExporter({
  url: 'http://localhost:4318/v1/traces'
});

const metricExporter = new OTLPMetricExporter({
  url: 'http://localhost:4318/v1/metrics'
});
```

### Jaeger
```typescript
import { JaegerExporter } from '@opentelemetry/exporter-jaeger';

const jaegerExporter = new JaegerExporter({
  endpoint: 'http://localhost:14268/api/traces'
});
```

### Console (Development)
```typescript
import { ConsoleSpanExporter } from '@opentelemetry/sdk-trace-base';

const consoleExporter = new ConsoleSpanExporter();
```

## Environment Variables

```bash
# OpenTelemetry Configuration
OTEL_SERVICE_NAME=scrapix-crawler
OTEL_SERVICE_VERSION=1.0.0
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer token

# Tracing
OTEL_TRACES_EXPORTER=otlp
OTEL_TRACES_SAMPLER=traceidratio
OTEL_TRACES_SAMPLER_ARG=0.1

# Metrics
OTEL_METRICS_EXPORTER=otlp
OTEL_METRIC_EXPORT_INTERVAL=30000
OTEL_METRIC_EXPORT_TIMEOUT=10000

# Logging
OTEL_LOG_LEVEL=info
```

## Dashboards and Alerts

Example Grafana queries:

```promql
# Request rate
rate(http_requests_total[5m])

# Error rate
rate(http_requests_total{status=~"5.."}[5m])

# P95 latency
histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m]))

# Active crawls
sum(crawler_active_crawls)

# Document processing rate
rate(documents_processed_total[5m])
```

## Best Practices

1. **Use semantic conventions** - Follow OpenTelemetry naming standards
2. **Add context** - Include relevant attributes in spans
3. **Sample wisely** - Balance observability with performance
4. **Handle errors** - Always record exceptions in spans
5. **Use baggage** - Propagate user/request context
6. **Batch exports** - Reduce overhead with batching
7. **Monitor overhead** - Track telemetry performance impact