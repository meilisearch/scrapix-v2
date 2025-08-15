import { v4 as uuidv4 } from 'uuid'
import SupabaseClientManager, {
  RunRow,
  RunInsert,
  RunUpdate,
  RunStatus,
} from './client'
import { Config } from '../types'
import {
  CrawlerCompletedEvent,
  ProgressUpdateEvent,
} from '../events/crawler-events'
import {
  WebhookManager,
  createWebhookManager,
  RunStartedPayload,
  RunCompletedPayload,
  RunFailedPayload,
  RunCancelledPayload,
  ProgressUpdatePayload,
} from '../webhook/webhook-manager'

/**
 * Interface for run creation options
 */
export interface CreateRunOptions {
  configId?: string
  configSnapshot?: Config
  createdBy?: string
  metadata?: Record<string, any>
}

/**
 * Interface for run metrics update
 */
export interface RunMetrics {
  totalPagesCrawled?: number
  totalPagesIndexed?: number
  totalDocumentsSent?: number
}

/**
 * Interface for run completion data
 */
export interface RunCompletionData {
  success: boolean
  totalPagesCrawled: number
  totalPagesIndexed: number
  totalDocumentsSent: number
  errorMessage?: string
  metadata?: Record<string, any>
}

/**
 * RunManager class handles the lifecycle of crawler runs in Supabase
 * Provides methods to create, update, complete, and track crawler runs
 * Includes integrated webhook notifications for run state changes
 */
export class RunManager {
  private client = SupabaseClientManager.getInstance()
  private runId?: string
  private webhookManager?: WebhookManager
  private config?: Config

  constructor(runId?: string, config?: Config) {
    this.runId = runId
    this.config = config

    // Initialize webhook manager if config provided
    if (config) {
      this.webhookManager = createWebhookManager(runId)
      try {
        this.webhookManager.registerFromConfig(config)
      } catch (error) {
        console.warn('Failed to register webhooks from config:', error)
      }
    }
  }

  /**
   * Create a new run in the database
   */
  public async createRun(options: CreateRunOptions = {}): Promise<string> {
    const runId = uuidv4()
    const now = new Date().toISOString()

    const runData: RunInsert = {
      id: runId,
      config_id: options.configId,
      config_snapshot: options.configSnapshot,
      status: 'pending',
      started_at: now,
      total_pages_crawled: 0,
      total_pages_indexed: 0,
      total_documents_sent: 0,
      created_by: options.createdBy,
      metadata: options.metadata,
    }

    const { error } = await this.client.from('runs').insert(runData)

    if (error) {
      throw new Error(`Failed to create run: ${error.message}`)
    }

    this.runId = runId
    return runId
  }

  /**
   * Update run status
   */
  public async updateStatus(
    status: RunStatus,
    errorMessage?: string
  ): Promise<void> {
    if (!this.runId) {
      throw new Error(
        'No run ID set. Create a run first or provide runId in constructor.'
      )
    }

    const updateData: RunUpdate = {
      status,
      ...(errorMessage && { error_message: errorMessage }),
    }

    const { error } = await this.client
      .from('runs')
      .update(updateData)
      .eq('id', this.runId)

    if (error) {
      throw new Error(`Failed to update run status: ${error.message}`)
    }
  }

  /**
   * Update run metrics (pages crawled, indexed, documents sent)
   */
  public async updateMetrics(metrics: RunMetrics): Promise<void> {
    if (!this.runId) {
      throw new Error(
        'No run ID set. Create a run first or provide runId in constructor.'
      )
    }

    const updateData: RunUpdate = {
      ...(metrics.totalPagesCrawled !== undefined && {
        total_pages_crawled: metrics.totalPagesCrawled,
      }),
      ...(metrics.totalPagesIndexed !== undefined && {
        total_pages_indexed: metrics.totalPagesIndexed,
      }),
      ...(metrics.totalDocumentsSent !== undefined && {
        total_documents_sent: metrics.totalDocumentsSent,
      }),
    }

    const { error } = await this.client
      .from('runs')
      .update(updateData)
      .eq('id', this.runId)

    if (error) {
      throw new Error(`Failed to update run metrics: ${error.message}`)
    }
  }

  /**
   * Update run progress from a progress event
   */
  public async updateProgressFromEvent(
    event: ProgressUpdateEvent
  ): Promise<void> {
    await this.updateMetrics({
      totalPagesCrawled: event.progress.crawled,
      totalPagesIndexed: event.progress.indexed,
      totalDocumentsSent: event.progress.documentsSent,
    })

    // Send progress webhook notification
    if (this.webhookManager && this.config) {
      try {
        const payload: ProgressUpdatePayload = {
          event: 'progress.update',
          timestamp: event.timestamp.toISOString(),
          runId: this.runId,
          meilisearch: {
            url: this.config.meilisearch_url,
            index: this.config.meilisearch_index_uid,
          },
          customData: this.config.webhook_payload || undefined,
          data: {
            progress: event.progress,
            completionPercentage: event.completionPercentage,
            estimatedTimeRemaining: event.estimatedTimeRemaining,
            currentRate: event.currentRate,
            avgPageTime: event.avgPageTime,
            memoryUsage: event.memoryUsage,
          },
        }

        await this.webhookManager.sendWebhook(payload)
      } catch (webhookError) {
        console.warn('Failed to send progress update webhook:', webhookError)
      }
    }
  }

  /**
   * Start a run (set status to running and record start time)
   */
  public async startRun(data?: {
    startUrls?: string[]
    crawlerType?: string
    featuresEnabled?: string[]
    estimatedTotal?: number
  }): Promise<void> {
    if (!this.runId) {
      throw new Error(
        'No run ID set. Create a run first or provide runId in constructor.'
      )
    }

    const updateData: RunUpdate = {
      status: 'running',
      started_at: new Date().toISOString(),
    }

    const { error } = await this.client
      .from('runs')
      .update(updateData)
      .eq('id', this.runId)

    if (error) {
      throw new Error(`Failed to start run: ${error.message}`)
    }

    // Send webhook notification
    if (this.webhookManager && this.config) {
      try {
        const payload: RunStartedPayload = {
          event: 'run.started',
          timestamp: new Date().toISOString(),
          runId: this.runId,
          meilisearch: {
            url: this.config.meilisearch_url,
            index: this.config.meilisearch_index_uid,
          },
          customData: this.config.webhook_payload || undefined,
          data: {
            startUrls: data?.startUrls || this.config.start_urls,
            crawlerType:
              data?.crawlerType || this.config.crawler_type || 'cheerio',
            featuresEnabled:
              data?.featuresEnabled || Object.keys(this.config.features || {}),
            estimatedTotal: data?.estimatedTotal,
            config: {
              maxConcurrency: this.config.max_concurrency || 10,
              maxRequestRetries: this.config.max_request_retries || 3,
              proxyEnabled: !!(
                this.config.proxy_configuration?.proxyUrls?.length ||
                this.config.proxy_configuration?.tieredProxyUrls?.length
              ),
            },
          },
        }

        await this.webhookManager.sendWebhook(payload)
      } catch (webhookError) {
        console.warn('Failed to send start run webhook:', webhookError)
      }
    }
  }

  /**
   * Complete a run successfully
   */
  public async completeRun(data: RunCompletionData): Promise<void> {
    if (!this.runId) {
      throw new Error(
        'No run ID set. Create a run first or provide runId in constructor.'
      )
    }

    // Get the current run to calculate duration
    const { data: currentRun, error: fetchError } = await this.client
      .from('runs')
      .select('started_at, metadata')
      .eq('id', this.runId)
      .single()

    if (fetchError) {
      throw new Error(`Failed to fetch run data: ${fetchError.message}`)
    }

    const now = new Date()
    const startTime = new Date(currentRun.started_at)
    const durationMs = now.getTime() - startTime.getTime()

    const updateData: RunUpdate = {
      status: data.success ? 'completed' : 'failed',
      completed_at: now.toISOString(),
      duration_ms: durationMs,
      total_pages_crawled: data.totalPagesCrawled,
      total_pages_indexed: data.totalPagesIndexed,
      total_documents_sent: data.totalDocumentsSent,
      error_message: data.errorMessage,
      ...(data.metadata && {
        metadata: {
          ...currentRun.metadata,
          ...data.metadata,
        },
      }),
    }

    const { error } = await this.client
      .from('runs')
      .update(updateData)
      .eq('id', this.runId)

    if (error) {
      throw new Error(`Failed to complete run: ${error.message}`)
    }

    // Send webhook notification
    if (this.webhookManager && this.config) {
      try {
        if (data.success) {
          const payload: RunCompletedPayload = {
            event: 'run.completed',
            timestamp: now.toISOString(),
            runId: this.runId,
            meilisearch: {
              url: this.config.meilisearch_url,
              index: this.config.meilisearch_index_uid,
            },
            customData: this.config.webhook_payload || undefined,
            data: {
              duration: durationMs,
              totalCrawled: data.totalPagesCrawled,
              totalIndexed: data.totalPagesIndexed,
              totalDocumentsSent: data.totalDocumentsSent,
              stats: data.metadata?.stats || {
                avgPageTime: 0,
                pagesPerMinute:
                  durationMs > 0
                    ? data.totalPagesCrawled / (durationMs / 60000)
                    : 0,
                successRate: 100,
                totalRetries: 0,
                totalErrors: 0,
              },
              memoryUsage: data.metadata?.memoryUsage,
            },
          }

          await this.webhookManager.sendWebhook(payload)
        } else {
          const payload: RunFailedPayload = {
            event: 'run.failed',
            timestamp: now.toISOString(),
            runId: this.runId,
            meilisearch: {
              url: this.config.meilisearch_url,
              index: this.config.meilisearch_index_uid,
            },
            customData: this.config.webhook_payload || undefined,
            data: {
              error: data.errorMessage || 'Run completed with failure',
              duration: durationMs,
              totalCrawled: data.totalPagesCrawled,
              totalIndexed: data.totalPagesIndexed,
              totalDocumentsSent: data.totalDocumentsSent,
            },
          }

          await this.webhookManager.sendWebhook(payload)
        }
      } catch (webhookError) {
        console.warn('Failed to send complete run webhook:', webhookError)
      }
    }
  }

  /**
   * Complete run from a crawler completed event
   */
  public async completeRunFromEvent(
    event: CrawlerCompletedEvent
  ): Promise<void> {
    await this.completeRun({
      success: event.success,
      totalPagesCrawled: event.totalCrawled,
      totalPagesIndexed: event.totalIndexed,
      totalDocumentsSent: event.totalDocumentsSent,
      errorMessage: event.error,
      metadata: {
        stats: event.stats,
        memoryUsage: event.memoryUsage,
      },
    })
  }

  /**
   * Fail a run with an error message
   */
  public async failRun(
    errorMessage: string,
    metadata?: Record<string, any>
  ): Promise<void> {
    if (!this.runId) {
      throw new Error(
        'No run ID set. Create a run first or provide runId in constructor.'
      )
    }

    // Get the current run to calculate duration and preserve existing data
    const { data: currentRun, error: fetchError } = await this.client
      .from('runs')
      .select(
        'started_at, total_pages_crawled, total_pages_indexed, total_documents_sent, metadata'
      )
      .eq('id', this.runId)
      .single()

    if (fetchError) {
      throw new Error(`Failed to fetch run data: ${fetchError.message}`)
    }

    const now = new Date()
    const startTime = new Date(currentRun.started_at)
    const durationMs = now.getTime() - startTime.getTime()

    const updateData: RunUpdate = {
      status: 'failed',
      completed_at: now.toISOString(),
      duration_ms: durationMs,
      error_message: errorMessage,
      ...(metadata && {
        metadata: {
          ...currentRun.metadata,
          ...metadata,
        },
      }),
    }

    const { error } = await this.client
      .from('runs')
      .update(updateData)
      .eq('id', this.runId)

    if (error) {
      throw new Error(`Failed to fail run: ${error.message}`)
    }

    // Send webhook notification
    if (this.webhookManager && this.config) {
      try {
        const payload: RunFailedPayload = {
          event: 'run.failed',
          timestamp: now.toISOString(),
          runId: this.runId,
          meilisearch: {
            url: this.config.meilisearch_url,
            index: this.config.meilisearch_index_uid,
          },
          customData: this.config.webhook_payload || undefined,
          data: {
            error: errorMessage,
            duration: durationMs,
            totalCrawled: currentRun.total_pages_crawled || 0,
            totalIndexed: currentRun.total_pages_indexed || 0,
            totalDocumentsSent: currentRun.total_documents_sent || 0,
            errorDetails: metadata?.stage
              ? {
                  stage: metadata.stage,
                  context: metadata.context,
                }
              : undefined,
          },
        }

        await this.webhookManager.sendWebhook(payload)
      } catch (webhookError) {
        console.warn('Failed to send fail run webhook:', webhookError)
      }
    }
  }

  /**
   * Cancel a run
   */
  public async cancelRun(reason?: string): Promise<void> {
    if (!this.runId) {
      throw new Error(
        'No run ID set. Create a run first or provide runId in constructor.'
      )
    }

    // Get current run data for webhook
    const { data: currentRun, error: fetchError } = await this.client
      .from('runs')
      .select(
        'started_at, total_pages_crawled, total_pages_indexed, total_documents_sent'
      )
      .eq('id', this.runId)
      .single()

    const now = new Date()
    const durationMs =
      currentRun && !fetchError
        ? now.getTime() - new Date(currentRun.started_at).getTime()
        : 0

    const updateData: RunUpdate = {
      status: 'cancelled',
      completed_at: now.toISOString(),
      duration_ms: durationMs,
      ...(reason && { error_message: reason }),
    }

    const { error } = await this.client
      .from('runs')
      .update(updateData)
      .eq('id', this.runId)

    if (error) {
      throw new Error(`Failed to cancel run: ${error.message}`)
    }

    // Send webhook notification
    if (this.webhookManager && this.config && currentRun && !fetchError) {
      try {
        const payload: RunCancelledPayload = {
          event: 'run.cancelled',
          timestamp: now.toISOString(),
          runId: this.runId,
          meilisearch: {
            url: this.config.meilisearch_url,
            index: this.config.meilisearch_index_uid,
          },
          customData: this.config.webhook_payload || undefined,
          data: {
            reason,
            duration: durationMs,
            totalCrawled: currentRun.total_pages_crawled || 0,
            totalIndexed: currentRun.total_pages_indexed || 0,
            totalDocumentsSent: currentRun.total_documents_sent || 0,
          },
        }

        await this.webhookManager.sendWebhook(payload)
      } catch (webhookError) {
        console.warn('Failed to send cancel run webhook:', webhookError)
      }
    }
  }

  /**
   * Get run details
   */
  public async getRun(runId?: string): Promise<RunRow | null> {
    const targetRunId = runId || this.runId
    if (!targetRunId) {
      throw new Error('No run ID provided')
    }

    const { data, error } = await this.client
      .from('runs')
      .select('*')
      .eq('id', targetRunId)
      .single()

    if (error) {
      if (error.code === 'PGRST116') {
        // No rows found
        return null
      }
      throw new Error(`Failed to get run: ${error.message}`)
    }

    return data
  }

  /**
   * Get runs with optional filtering
   */
  public async getRuns(
    options: {
      configId?: string
      status?: RunStatus
      createdBy?: string
      limit?: number
      offset?: number
      orderBy?: 'started_at' | 'completed_at' | 'duration_ms'
      ascending?: boolean
    } = {}
  ): Promise<RunRow[]> {
    let query = this.client.from('runs').select('*')

    if (options.configId) {
      query = query.eq('config_id', options.configId)
    }

    if (options.status) {
      query = query.eq('status', options.status)
    }

    if (options.createdBy) {
      query = query.eq('created_by', options.createdBy)
    }

    if (options.orderBy) {
      query = query.order(options.orderBy, {
        ascending: options.ascending ?? false,
      })
    } else {
      query = query.order('started_at', { ascending: false })
    }

    if (options.limit) {
      query = query.limit(options.limit)
    }

    if (options.offset) {
      query = query.range(
        options.offset,
        options.offset + (options.limit || 100) - 1
      )
    }

    const { data, error } = await query

    if (error) {
      throw new Error(`Failed to get runs: ${error.message}`)
    }

    return data || []
  }

  /**
   * Get current run ID
   */
  public getRunId(): string | undefined {
    return this.runId
  }

  /**
   * Set run ID (useful when working with existing runs)
   */
  public setRunId(runId: string): void {
    this.runId = runId
  }

  /**
   * Static method to create a new run manager with a new run
   */
  public static async createWithNewRun(
    options: CreateRunOptions = {},
    config?: Config
  ): Promise<RunManager> {
    const manager = new RunManager(undefined, config)
    await manager.createRun(options)
    return manager
  }

  /**
   * Static method to get run manager for existing run
   */
  public static forRun(runId: string, config?: Config): RunManager {
    return new RunManager(runId, config)
  }

  /**
   * Set webhook manager configuration
   */
  public setConfig(config: Config): void {
    this.config = config
    this.webhookManager = createWebhookManager(this.runId)
    try {
      this.webhookManager.registerFromConfig(config)
    } catch (error) {
      console.warn('Failed to register webhooks from config:', error)
    }
  }

  /**
   * Get webhook manager (for advanced webhook operations)
   */
  public getWebhookManager(): WebhookManager | undefined {
    return this.webhookManager
  }
}
