import { Config } from './types'
import { Log } from 'crawlee'
import {
  WebhookManager,
  createWebhookManager,
  RunStartedPayload,
  RunCompletedPayload,
  RunFailedPayload,
  RunPausedPayload,
  ProgressUpdatePayload,
} from './webhook/webhook-manager'

const log = new Log({ prefix: 'WebhookNotifier' })

// This webhook sender is a singleton that wraps the new WebhookManager
export class Webhook {
  private static instance: Webhook
  private webhookManager: WebhookManager

  configured = false

  constructor(config: Config) {
    this.webhookManager = createWebhookManager()

    log.info('Initializing WebhookNotifier', {
      webhookConfigured: !!(
        config.webhook_url ||
        process.env.WEBHOOK_URL ||
        config.webhooks
      ),
    })

    // Register webhooks from config
    try {
      this.webhookManager.registerFromConfig(config)
      this.configured = this.webhookManager
        .getWebhookStatus()
        .some((w) => w.enabled)
    } catch (error) {
      log.error('Failed to register webhooks from config', { error })
      this.configured = false
    }

    if (!this.configured) {
      log.warning(
        'WebhookNotifier not configured. Set WEBHOOK_URL environment variable, provide webhook_url in config, or configure webhooks for notifications.'
      )
    }
  }

  public static get(config: Config): Webhook {
    if (!Webhook.instance) {
      Webhook.instance = new Webhook(config)
    }
    return Webhook.instance
  }

  /**
   * Set the run ID for webhook payloads
   */
  public setRunId(runId: string): void {
    this.webhookManager.setRunId(runId)
  }

  /**
   * Get the underlying webhook manager for advanced usage
   */
  public getManager(): WebhookManager {
    return this.webhookManager
  }

  async started(
    config: Config,
    data?: {
      startUrls?: string[]
      crawlerType?: string
      featuresEnabled?: string[]
      estimatedTotal?: number
    }
  ) {
    if (!this.configured) return

    const payload: RunStartedPayload = {
      event: 'run.started',
      timestamp: new Date().toISOString(),
      meilisearch: {
        url: config.meilisearch_url,
        index: config.meilisearch_index_uid,
      },
      customData: config.webhook_payload || undefined,
      data: {
        startUrls: data?.startUrls || config.start_urls,
        crawlerType: data?.crawlerType || config.crawler_type || 'cheerio',
        featuresEnabled:
          data?.featuresEnabled || Object.keys(config.features || {}),
        estimatedTotal: data?.estimatedTotal,
        config: {
          maxConcurrency: config.max_concurrency || 10,
          maxRequestRetries: config.max_request_retries || 3,
          proxyEnabled: !!(
            config.proxy_configuration?.proxyUrls?.length ||
            config.proxy_configuration?.tieredProxyUrls?.length
          ),
        },
      },
    }

    try {
      await this.webhookManager.sendWebhook(payload)
    } catch (error) {
      log.error('Failed to send started webhook', { error })
    }
  }

  async active(config: Config, data: Record<string, any>) {
    if (!this.configured) return

    // Convert legacy 'active' status to progress update
    const payload: ProgressUpdatePayload = {
      event: 'progress.update',
      timestamp: new Date().toISOString(),
      meilisearch: {
        url: config.meilisearch_url,
        index: config.meilisearch_index_uid,
      },
      customData: config.webhook_payload || undefined,
      data: {
        progress: {
          crawled: data.totalCrawled || 0,
          indexed: data.totalIndexed || 0,
          documentsSent: data.totalDocumentsSent || 0,
          errors: data.totalErrors || 0,
        },
        completionPercentage: data.completionPercentage,
        estimatedTimeRemaining: data.estimatedTimeRemaining,
        currentRate: data.currentRate || 0,
        avgPageTime: data.avgPageTime || 0,
        memoryUsage: data.memoryUsage,
      },
    }

    try {
      await this.webhookManager.sendWebhook(payload)
    } catch (error) {
      log.error('Failed to send progress update webhook', { error })
    }
  }

  async paused(config: Config, reason = 'Manual pause') {
    if (!this.configured) return

    const payload: RunPausedPayload = {
      event: 'run.paused',
      timestamp: new Date().toISOString(),
      meilisearch: {
        url: config.meilisearch_url,
        index: config.meilisearch_index_uid,
      },
      customData: config.webhook_payload || undefined,
      data: {
        reason,
        progress: {
          crawled: 0, // These would need to be passed in or tracked
          indexed: 0,
          documentsSent: 0,
        },
      },
    }

    try {
      await this.webhookManager.sendWebhook(payload)
    } catch (error) {
      log.error('Failed to send paused webhook', { error })
    }
  }

  async completed(
    config: Config,
    data: {
      nbDocumentsSent: number
      totalCrawled?: number
      totalIndexed?: number
      duration?: number
      stats?: any
      memoryUsage?: NodeJS.MemoryUsage
    }
  ) {
    if (!this.configured) return

    const payload: RunCompletedPayload = {
      event: 'run.completed',
      timestamp: new Date().toISOString(),
      meilisearch: {
        url: config.meilisearch_url,
        index: config.meilisearch_index_uid,
      },
      customData: config.webhook_payload || undefined,
      data: {
        duration: data.duration || 0,
        totalCrawled: data.totalCrawled || 0,
        totalIndexed: data.totalIndexed || 0,
        totalDocumentsSent: data.nbDocumentsSent,
        stats: data.stats || {
          avgPageTime: 0,
          pagesPerMinute: 0,
          successRate: 100,
          totalRetries: 0,
          totalErrors: 0,
        },
        memoryUsage: data.memoryUsage,
      },
    }

    try {
      await this.webhookManager.sendWebhook(payload)
    } catch (error) {
      log.error('Failed to send completed webhook', { error })
    }
  }

  async failed(
    config: Config,
    error: Error,
    data?: {
      totalCrawled?: number
      totalIndexed?: number
      totalDocumentsSent?: number
      duration?: number
      stage?: string
      context?: Record<string, any>
    }
  ) {
    if (!this.configured) return

    const payload: RunFailedPayload = {
      event: 'run.failed',
      timestamp: new Date().toISOString(),
      meilisearch: {
        url: config.meilisearch_url,
        index: config.meilisearch_index_uid,
      },
      customData: config.webhook_payload || undefined,
      data: {
        error: error.message,
        duration: data?.duration,
        totalCrawled: data?.totalCrawled || 0,
        totalIndexed: data?.totalIndexed || 0,
        totalDocumentsSent: data?.totalDocumentsSent || 0,
        errorDetails: data?.stage
          ? {
              stage: data.stage,
              context: data.context,
            }
          : undefined,
      },
    }

    try {
      await this.webhookManager.sendWebhook(payload)
    } catch (error) {
      log.error('Failed to send failed webhook', { error })
    }
  }

  /**
   * @deprecated Use the specific methods instead. This is kept for backward compatibility.
   */
  async __callWebhook(config: Config, data: any) {
    log.warning(
      '__callWebhook is deprecated, use specific webhook methods instead'
    )

    // Try to map old data format to new webhook events
    switch (data.status) {
      case 'started':
        await this.started(config)
        break
      case 'completed':
        await this.completed(config, {
          nbDocumentsSent: data.nb_documents_sent || 0,
        })
        break
      case 'failed':
        await this.failed(config, new Error(data.error || 'Unknown error'))
        break
      case 'paused':
        await this.paused(config)
        break
      case 'active':
        await this.active(config, data)
        break
      default:
        log.warning('Unknown webhook status', { status: data.status })
    }
  }
}
