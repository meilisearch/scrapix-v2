import { NodeSDK } from '@opentelemetry/sdk-node'
import { getNodeAutoInstrumentations } from '@opentelemetry/auto-instrumentations-node'
import { resourceFromAttributes } from '@opentelemetry/resources'
import { 
  SEMRESATTRS_SERVICE_NAME,
  SEMRESATTRS_SERVICE_VERSION,
  SEMRESATTRS_DEPLOYMENT_ENVIRONMENT
} from '@opentelemetry/semantic-conventions'
import { PeriodicExportingMetricReader, MeterProvider, ConsoleMetricExporter } from '@opentelemetry/sdk-metrics'
import { BatchSpanProcessor, ConsoleSpanExporter } from '@opentelemetry/sdk-trace-node'
import { OTLPTraceExporter } from '@opentelemetry/exporter-trace-otlp-http'
import { OTLPMetricExporter } from '@opentelemetry/exporter-metrics-otlp-http'
import { trace, metrics, context, SpanStatusCode, SpanKind, Span } from '@opentelemetry/api'
import { Config } from '../types'

// Customer identification utilities
export function extractCustomerId(config: Config): string {
  // Try to extract from Meilisearch URL
  if (config.meilisearch_url) {
    const url = new URL(config.meilisearch_url)
    // Extract subdomain if exists (e.g., customer1.meilisearch.io)
    const parts = url.hostname.split('.')
    if (parts.length > 2) {
      return parts[0]
    }
    // Use hostname as fallback
    return url.hostname.replace(/\./g, '_')
  }
  return 'unknown'
}

export function extractCustomerAttributes(config: Config): Record<string, any> {
  const customerId = extractCustomerId(config)
  return {
    'customer.id': customerId,
    'customer.meilisearch_url': config.meilisearch_url,
    'customer.index': config.meilisearch_index_uid,
    'config.crawler_type': config.crawler_type || 'cheerio',
    'config.max_pages': (config as any).max_pages_to_crawl || -1,
    'config.features': Object.keys(config).filter(k => k.includes('_enabled') && config[k as keyof Config]).join(','),
  }
}

// Telemetry singleton
class TelemetryManager {
  private static instance: TelemetryManager
  private sdk?: NodeSDK
  private isInitialized = false
  private _tracer = trace.getTracer('scrapix', '1.0.0')
  private _meter = metrics.getMeter('scrapix', '1.0.0')

  private constructor() {}

  static getInstance(): TelemetryManager {
    if (!TelemetryManager.instance) {
      TelemetryManager.instance = new TelemetryManager()
    }
    return TelemetryManager.instance
  }

  async initialize(serviceName: string = 'scrapix-scraper') {
    if (this.isInitialized) return

    const otlpEndpoint = process.env.OTEL_EXPORTER_OTLP_ENDPOINT || 'http://localhost:4318'
    const environment = process.env.NODE_ENV || 'development'
    const isDevelopment = environment === 'development'

    // Resource configuration
    const resource = resourceFromAttributes({
      [SEMRESATTRS_SERVICE_NAME]: serviceName,
      [SEMRESATTRS_SERVICE_VERSION]: process.env.npm_package_version || '1.0.0',
      [SEMRESATTRS_DEPLOYMENT_ENVIRONMENT]: environment,
    })

    // Trace exporter
    const traceExporter = isDevelopment
      ? new ConsoleSpanExporter()
      : new OTLPTraceExporter({
          url: `${otlpEndpoint}/v1/traces`,
          headers: process.env.OTEL_EXPORTER_OTLP_HEADERS ? 
            JSON.parse(process.env.OTEL_EXPORTER_OTLP_HEADERS) : {},
        })

    // Metric exporter
    const metricExporter = isDevelopment
      ? new ConsoleMetricExporter()
      : new OTLPMetricExporter({
          url: `${otlpEndpoint}/v1/metrics`,
          headers: process.env.OTEL_EXPORTER_OTLP_HEADERS ? 
            JSON.parse(process.env.OTEL_EXPORTER_OTLP_HEADERS) : {},
        })

    // Configure SDK
    this.sdk = new NodeSDK({
      resource,
      spanProcessor: new BatchSpanProcessor(traceExporter),
      instrumentations: [
        getNodeAutoInstrumentations({
          '@opentelemetry/instrumentation-fs': {
            enabled: false, // Disable fs instrumentation to reduce noise
          },
        }),
      ],
    })

    // Initialize metrics separately
    const meterProvider = new MeterProvider({
      resource,
      readers: [
        new PeriodicExportingMetricReader({
          exporter: metricExporter,
          exportIntervalMillis: 10000, // Export every 10 seconds
        }),
      ],
    })
    metrics.setGlobalMeterProvider(meterProvider)

    // Start SDK
    await this.sdk.start()
    this.isInitialized = true
    
    console.log(`OpenTelemetry initialized for ${serviceName} in ${environment} mode`)
  }

  async shutdown() {
    if (this.sdk) {
      await this.sdk.shutdown()
      this.isInitialized = false
    }
  }

  get tracer() {
    return this._tracer
  }

  get meter() {
    return this._meter
  }

  // Helper method to start a span with common attributes
  startSpan(name: string, config?: Config, attributes?: Record<string, any>): Span {
    const spanAttributes = {
      ...attributes,
      ...(config ? extractCustomerAttributes(config) : {}),
    }
    
    return this.tracer.startSpan(name, {
      attributes: spanAttributes,
      kind: SpanKind.INTERNAL,
    })
  }

  // Helper for async operations with automatic span management
  async withSpan<T>(
    name: string,
    config: Config | undefined,
    fn: (span: Span) => Promise<T>,
    attributes?: Record<string, any>
  ): Promise<T> {
    const span = this.startSpan(name, config, attributes)
    
    try {
      const result = await context.with(
        trace.setSpan(context.active(), span),
        () => fn(span)
      )
      span.setStatus({ code: SpanStatusCode.OK })
      return result
    } catch (error) {
      span.recordException(error as Error)
      span.setStatus({
        code: SpanStatusCode.ERROR,
        message: (error as Error).message,
      })
      throw error
    } finally {
      span.end()
    }
  }
}

// Export singleton instance
export const telemetry = TelemetryManager.getInstance()

// Business metrics helpers
export class BusinessMetrics {
  private crawlsCounter = telemetry.meter.createCounter('scraper.customer.crawls.total', {
    description: 'Total number of crawls per customer',
  })

  private pagesCounter = telemetry.meter.createCounter('scraper.customer.pages.crawled', {
    description: 'Total pages crawled per customer',
  })

  private documentsCounter = telemetry.meter.createCounter('scraper.customer.documents.indexed', {
    description: 'Total documents indexed per customer',
  })

  private openaiTokensCounter = telemetry.meter.createCounter('scraper.openai.tokens.used', {
    description: 'OpenAI tokens consumed',
  })

  private openaiCostCounter = telemetry.meter.createCounter('scraper.openai.cost.estimated', {
    description: 'Estimated OpenAI cost in USD cents',
  })

  private featureUsageCounter = telemetry.meter.createCounter('scraper.customer.features.usage', {
    description: 'Feature usage per customer',
  })

  private crawlDurationHistogram = telemetry.meter.createHistogram('scraper.customer.crawl.duration', {
    description: 'Crawl duration in seconds per customer',
    unit: 's',
  })

  recordCrawlStart(config: Config) {
    const customerId = extractCustomerId(config)
    this.crawlsCounter.add(1, {
      'customer.id': customerId,
      'customer.meilisearch_url': config.meilisearch_url,
      'crawler.type': config.crawler_type || 'cheerio',
    })
  }

  recordPageCrawled(config: Config, success: boolean) {
    const customerId = extractCustomerId(config)
    this.pagesCounter.add(1, {
      'customer.id': customerId,
      'success': success,
    })
  }

  recordDocumentsIndexed(config: Config, count: number) {
    const customerId = extractCustomerId(config)
    this.documentsCounter.add(count, {
      'customer.id': customerId,
      'index': config.meilisearch_index_uid,
    })
  }

  recordOpenAIUsage(
    config: Config, 
    model: string, 
    inputTokens: number, 
    outputTokens: number,
    operation: 'extraction' | 'summary'
  ) {
    const customerId = extractCustomerId(config)
    const attributes = {
      'customer.id': customerId,
      'model': model,
      'operation': operation,
    }

    // Record tokens
    this.openaiTokensCounter.add(inputTokens, { ...attributes, 'token.type': 'input' })
    this.openaiTokensCounter.add(outputTokens, { ...attributes, 'token.type': 'output' })

    // Estimate cost (prices in cents per 1K tokens as of 2024)
    const pricing: Record<string, { input: number, output: number }> = {
      'gpt-4': { input: 3, output: 6 },
      'gpt-4-turbo': { input: 1, output: 3 },
      'gpt-3.5-turbo': { input: 0.05, output: 0.15 },
    }

    const modelPricing = pricing[model] || pricing['gpt-3.5-turbo']
    const estimatedCost = (inputTokens * modelPricing.input + outputTokens * modelPricing.output) / 1000

    this.openaiCostCounter.add(estimatedCost, attributes)
  }

  recordFeatureUsage(config: Config, feature: string) {
    const customerId = extractCustomerId(config)
    this.featureUsageCounter.add(1, {
      'customer.id': customerId,
      'feature': feature,
    })
  }

  recordCrawlDuration(config: Config, durationSeconds: number) {
    const customerId = extractCustomerId(config)
    this.crawlDurationHistogram.record(durationSeconds, {
      'customer.id': customerId,
      'crawler.type': config.crawler_type || 'cheerio',
    })
  }
}

export const businessMetrics = new BusinessMetrics()