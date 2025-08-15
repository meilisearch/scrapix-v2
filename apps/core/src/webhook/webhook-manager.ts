import axios, { AxiosResponse, AxiosError } from 'axios'
import { Log } from 'crawlee'
import crypto from 'crypto'
import { Config } from '../types'

const log = new Log({ prefix: 'WebhookManager' })

/**
 * Webhook configuration interface
 */
export interface WebhookConfig {
  /** Webhook URL to send events to */
  url: string
  /** Authentication method */
  auth?: {
    /** Bearer token authentication */
    bearer?: string
    /** HMAC signature authentication */
    hmac?: {
      secret: string
      algorithm: 'sha256' | 'sha512'
      header: string
    }
    /** Custom headers */
    headers?: Record<string, string>
  }
  /** Which events to send to this webhook */
  events: WebhookEventType[]
  /** Whether this webhook is enabled */
  enabled: boolean
  /** Custom timeout in milliseconds */
  timeout?: number
  /** Custom webhook name/identifier */
  name?: string
}

/**
 * Event types that can be sent via webhooks
 */
export type WebhookEventType =
  | 'run.started'
  | 'run.completed'
  | 'run.failed'
  | 'run.cancelled'
  | 'run.paused'
  | 'run.resumed'
  | 'progress.update'
  | 'page.crawled'
  | 'page.indexed'
  | 'page.error'
  | 'batch.sent'

/**
 * Base webhook payload interface
 */
export interface BaseWebhookPayload {
  /** Event type */
  event: WebhookEventType
  /** Event timestamp */
  timestamp: string
  /** Run ID */
  runId?: string
  /** Meilisearch configuration */
  meilisearch: {
    url: string
    index: string
  }
  /** Custom payload data from config */
  customData?: Record<string, any>
}

/**
 * Run state webhook payloads
 */
export interface RunStartedPayload extends BaseWebhookPayload {
  event: 'run.started'
  data: {
    startUrls: string[]
    crawlerType: string
    featuresEnabled: string[]
    estimatedTotal?: number
    config: {
      maxConcurrency: number
      maxRequestRetries: number
      proxyEnabled: boolean
    }
  }
}

export interface RunCompletedPayload extends BaseWebhookPayload {
  event: 'run.completed'
  data: {
    duration: number
    totalCrawled: number
    totalIndexed: number
    totalDocumentsSent: number
    stats: {
      avgPageTime: number
      pagesPerMinute: number
      successRate: number
      totalRetries: number
      totalErrors: number
    }
    memoryUsage?: NodeJS.MemoryUsage
  }
}

export interface RunFailedPayload extends BaseWebhookPayload {
  event: 'run.failed'
  data: {
    error: string
    duration?: number
    totalCrawled: number
    totalIndexed: number
    totalDocumentsSent: number
    errorDetails?: {
      stage: string
      context?: Record<string, any>
    }
  }
}

export interface RunCancelledPayload extends BaseWebhookPayload {
  event: 'run.cancelled'
  data: {
    reason?: string
    duration?: number
    totalCrawled: number
    totalIndexed: number
    totalDocumentsSent: number
  }
}

export interface RunPausedPayload extends BaseWebhookPayload {
  event: 'run.paused'
  data: {
    reason: string
    progress: {
      crawled: number
      indexed: number
      documentsSent: number
    }
  }
}

export interface RunResumedPayload extends BaseWebhookPayload {
  event: 'run.resumed'
  data: {
    pauseDuration: number
    progress: {
      crawled: number
      indexed: number
      documentsSent: number
    }
  }
}

export interface ProgressUpdatePayload extends BaseWebhookPayload {
  event: 'progress.update'
  data: {
    progress: {
      crawled: number
      indexed: number
      documentsSent: number
      errors: number
    }
    completionPercentage?: number
    estimatedTimeRemaining?: number
    currentRate: number
    avgPageTime: number
    memoryUsage?: NodeJS.MemoryUsage
  }
}

export interface PageCrawledPayload extends BaseWebhookPayload {
  event: 'page.crawled'
  data: {
    url: string
    success: boolean
    duration?: number
    depth?: number
    crawlerType: string
    error?: string
    totalCrawled: number
    totalIndexed: number
  }
}

export interface PageIndexedPayload extends BaseWebhookPayload {
  event: 'page.indexed'
  data: {
    url: string
    success: boolean
    duration?: number
    documentCount: number
    featuresApplied: string[]
    error?: string
    totalIndexed: number
  }
}

export interface PageErrorPayload extends BaseWebhookPayload {
  event: 'page.error'
  data: {
    url: string
    error: string
    stage: 'crawling' | 'indexing' | 'processing'
    isRetry?: boolean
    retryAttempt?: number
    crawlerType: string
    context?: Record<string, any>
  }
}

export interface BatchSentPayload extends BaseWebhookPayload {
  event: 'batch.sent'
  data: {
    documentCount: number
    success: boolean
    duration?: number
    indexUid: string
    taskId?: number
    batchSizeBytes?: number
    error?: string
    totalDocumentsSent: number
    isRetry?: boolean
    retryAttempt?: number
  }
}

/**
 * Union type for all webhook payloads
 */
export type WebhookPayload =
  | RunStartedPayload
  | RunCompletedPayload
  | RunFailedPayload
  | RunCancelledPayload
  | RunPausedPayload
  | RunResumedPayload
  | ProgressUpdatePayload
  | PageCrawledPayload
  | PageIndexedPayload
  | PageErrorPayload
  | BatchSentPayload

/**
 * Retry configuration
 */
export interface RetryConfig {
  /** Maximum number of retry attempts */
  maxAttempts: number
  /** Base delay in milliseconds */
  baseDelay: number
  /** Maximum delay in milliseconds */
  maxDelay: number
  /** Exponential backoff multiplier */
  backoffMultiplier: number
  /** Jitter to add randomness to delays */
  jitter: boolean
}

/**
 * Webhook delivery attempt result
 */
export interface WebhookDeliveryResult {
  /** Whether the delivery was successful */
  success: boolean
  /** HTTP status code */
  statusCode?: number
  /** Response body (if available) */
  response?: any
  /** Error message if failed */
  error?: string
  /** Number of attempts made */
  attempts: number
  /** Total time taken in milliseconds */
  duration: number
}

/**
 * Enhanced webhook manager with retry logic, authentication, and event filtering
 */
export class WebhookManager {
  private webhooks: Map<string, WebhookConfig> = new Map()
  private defaultRetryConfig: RetryConfig = {
    maxAttempts: 3,
    baseDelay: 1000,
    maxDelay: 30000,
    backoffMultiplier: 2,
    jitter: true,
  }

  constructor(private runId?: string) {}

  /**
   * Register a webhook configuration
   */
  public registerWebhook(id: string, config: WebhookConfig): void {
    if (!config.url) {
      throw new Error('Webhook URL is required')
    }

    if (!config.events || config.events.length === 0) {
      throw new Error('At least one event type must be specified')
    }

    this.webhooks.set(id, config)

    log.info('Webhook registered', {
      id,
      url: config.url,
      events: config.events,
      enabled: config.enabled,
      name: config.name,
    })
  }

  /**
   * Register webhooks from config
   */
  public registerFromConfig(config: Config): void {
    // Register primary webhook from legacy config
    if (config.webhook_url || process.env.WEBHOOK_URL) {
      this.registerWebhook('primary', {
        url: config.webhook_url || process.env.WEBHOOK_URL!,
        events: [
          'run.started',
          'run.completed',
          'run.failed',
          'progress.update',
        ],
        enabled: true,
        auth: process.env.WEBHOOK_TOKEN
          ? {
              bearer: process.env.WEBHOOK_TOKEN,
            }
          : undefined,
        name: 'Primary Webhook',
      })
    }

    // Register additional webhooks from enhanced config
    if (config.webhooks) {
      for (const [id, webhookConfig] of Object.entries(config.webhooks)) {
        this.registerWebhook(id, webhookConfig as WebhookConfig)
      }
    }
  }

  /**
   * Remove a webhook
   */
  public unregisterWebhook(id: string): void {
    this.webhooks.delete(id)
    log.info('Webhook unregistered', { id })
  }

  /**
   * Send a webhook payload to all registered webhooks that accept the event type
   */
  public async sendWebhook(
    payload: WebhookPayload
  ): Promise<Map<string, WebhookDeliveryResult>> {
    const results = new Map<string, WebhookDeliveryResult>()

    // Add run ID if available
    if (this.runId) {
      payload.runId = this.runId
    }

    const enabledWebhooks = Array.from(this.webhooks.entries()).filter(
      ([_, config]) => config.enabled && config.events.includes(payload.event)
    )

    if (enabledWebhooks.length === 0) {
      log.debug('No webhooks registered for event', { event: payload.event })
      return results
    }

    // Send to all matching webhooks concurrently
    const deliveryPromises = enabledWebhooks.map(async ([id, config]) => {
      const result = await this.deliverWebhook(id, config, payload)
      results.set(id, result)
      return { id, result }
    })

    await Promise.allSettled(deliveryPromises)

    const successCount = Array.from(results.values()).filter(
      (r) => r.success
    ).length
    const totalCount = results.size

    log.info('Webhook delivery completed', {
      event: payload.event,
      successCount,
      totalCount,
      runId: this.runId,
    })

    return results
  }

  /**
   * Deliver a webhook with retry logic
   */
  private async deliverWebhook(
    id: string,
    config: WebhookConfig,
    payload: WebhookPayload,
    retryConfig: RetryConfig = this.defaultRetryConfig
  ): Promise<WebhookDeliveryResult> {
    const startTime = Date.now()
    let lastError: string = ''
    let attempts = 0

    for (let attempt = 1; attempt <= retryConfig.maxAttempts; attempt++) {
      attempts = attempt

      try {
        const response = await this.sendHttpRequest(config, payload)

        const duration = Date.now() - startTime
        const result: WebhookDeliveryResult = {
          success: true,
          statusCode: response.status,
          response: response.data,
          attempts,
          duration,
        }

        if (attempt > 1) {
          log.info('Webhook delivery succeeded after retries', {
            id,
            attempts,
            duration,
            event: payload.event,
          })
        }

        return result
      } catch (error) {
        lastError = this.extractErrorMessage(error)

        log.warning('Webhook delivery attempt failed', {
          id,
          attempt,
          maxAttempts: retryConfig.maxAttempts,
          error: lastError,
          event: payload.event,
        })

        // Don't retry on client errors (4xx), except 429 (rate limit)
        if (error instanceof AxiosError) {
          const status = error.response?.status
          if (status && status >= 400 && status < 500 && status !== 429) {
            log.error(
              'Webhook delivery failed with client error, not retrying',
              {
                id,
                status,
                error: lastError,
                event: payload.event,
              }
            )
            break
          }
        }

        // Calculate delay for next attempt
        if (attempt < retryConfig.maxAttempts) {
          const delay = this.calculateRetryDelay(attempt, retryConfig)
          log.debug('Waiting before retry', {
            id,
            delay,
            nextAttempt: attempt + 1,
          })
          await this.sleep(delay)
        }
      }
    }

    const duration = Date.now() - startTime
    return {
      success: false,
      error: lastError,
      attempts,
      duration,
    }
  }

  /**
   * Send HTTP request with authentication
   */
  private async sendHttpRequest(
    config: WebhookConfig,
    payload: WebhookPayload
  ): Promise<AxiosResponse> {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
      'User-Agent': 'Scrapix-Webhook/1.0',
      ...config.auth?.headers,
    }

    // Add bearer token authentication
    if (config.auth?.bearer) {
      headers['Authorization'] = `Bearer ${config.auth.bearer}`
    }

    // Add HMAC signature authentication
    if (config.auth?.hmac) {
      const signature = this.generateHmacSignature(payload, config.auth.hmac)
      headers[config.auth.hmac.header] = signature
    }

    const timeout = config.timeout || 30000

    log.debug('Sending webhook request', {
      url: config.url,
      event: payload.event,
      timeout,
      hasAuth: !!config.auth,
    })

    return axios.post(config.url, payload, {
      headers,
      timeout,
      validateStatus: (status) => status >= 200 && status < 300,
    })
  }

  /**
   * Generate HMAC signature for payload
   */
  private generateHmacSignature(
    payload: WebhookPayload,
    hmacConfig: NonNullable<WebhookConfig['auth']>['hmac']
  ): string {
    if (!hmacConfig) {
      throw new Error('HMAC config is required')
    }

    const payloadString = JSON.stringify(payload)
    const hash = crypto
      .createHmac(hmacConfig.algorithm, hmacConfig.secret)
      .update(payloadString, 'utf8')
      .digest('hex')

    return `${hmacConfig.algorithm}=${hash}`
  }

  /**
   * Calculate retry delay with exponential backoff and jitter
   */
  private calculateRetryDelay(attempt: number, config: RetryConfig): number {
    const exponentialDelay =
      config.baseDelay * Math.pow(config.backoffMultiplier, attempt - 1)
    const cappedDelay = Math.min(exponentialDelay, config.maxDelay)

    if (config.jitter) {
      // Add random jitter (±25% of delay)
      const jitterRange = cappedDelay * 0.25
      const jitter = (Math.random() - 0.5) * 2 * jitterRange
      return Math.max(0, cappedDelay + jitter)
    }

    return cappedDelay
  }

  /**
   * Extract error message from various error types
   */
  private extractErrorMessage(error: unknown): string {
    if (error instanceof AxiosError) {
      if (error.response) {
        return `HTTP ${error.response.status}: ${error.response.statusText} - ${JSON.stringify(error.response.data)}`
      } else if (error.request) {
        return `Network error: ${error.message}`
      } else {
        return `Request setup error: ${error.message}`
      }
    } else if (error instanceof Error) {
      return error.message
    } else {
      return String(error)
    }
  }

  /**
   * Sleep utility function
   */
  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms))
  }

  /**
   * Get webhook status
   */
  public getWebhookStatus(): Array<{
    id: string
    url: string
    enabled: boolean
    events: WebhookEventType[]
    name?: string
  }> {
    return Array.from(this.webhooks.entries()).map(([id, config]) => ({
      id,
      url: config.url,
      enabled: config.enabled,
      events: config.events,
      name: config.name,
    }))
  }

  /**
   * Enable/disable a webhook
   */
  public setWebhookEnabled(id: string, enabled: boolean): void {
    const webhook = this.webhooks.get(id)
    if (webhook) {
      webhook.enabled = enabled
      log.info('Webhook status changed', { id, enabled })
    } else {
      throw new Error(`Webhook with id '${id}' not found`)
    }
  }

  /**
   * Clear all webhooks
   */
  public clearWebhooks(): void {
    this.webhooks.clear()
    log.info('All webhooks cleared')
  }

  /**
   * Set run ID for all future webhook payloads
   */
  public setRunId(runId: string): void {
    this.runId = runId
  }

  /**
   * Get run ID
   */
  public getRunId(): string | undefined {
    return this.runId
  }
}

/**
 * Create a new webhook manager instance
 */
export function createWebhookManager(runId?: string): WebhookManager {
  return new WebhookManager(runId)
}
