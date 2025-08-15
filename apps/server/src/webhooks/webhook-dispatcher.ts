import Queue from 'bull'
import Redis from 'ioredis'
import { Log } from 'crawlee'
import {
  WebhookManager,
  WebhookPayload,
  WebhookDeliveryResult,
  WebhookConfig,
  createWebhookManager,
} from '../../../core/src/webhook/webhook-manager'

const log = new Log({ prefix: 'WebhookDispatcher' })

/**
 * Webhook job data for the queue
 */
interface WebhookJob {
  /** Unique job ID */
  id: string
  /** Run ID associated with this webhook */
  runId?: string
  /** Webhook payload to send */
  payload: WebhookPayload
  /** Webhook configurations to use (overrides registered webhooks if provided) */
  webhookConfigs?: Record<string, WebhookConfig>
  /** Whether to use only the provided configs (ignore registered webhooks) */
  useOnlyProvidedConfigs?: boolean
  /** Custom retry configuration for this job */
  retryConfig?: {
    attempts: number
    delay: number
    backoff: 'fixed' | 'exponential'
  }
}

/**
 * Webhook delivery status tracking
 */
interface WebhookDeliveryStatus {
  /** Job ID */
  jobId: string
  /** Run ID */
  runId?: string
  /** Event type */
  event: string
  /** Job status */
  status: 'pending' | 'processing' | 'completed' | 'failed'
  /** Timestamp when job was created */
  createdAt: Date
  /** Timestamp when job was processed */
  processedAt?: Date
  /** Timestamp when job was completed */
  completedAt?: Date
  /** Number of delivery attempts made */
  attempts: number
  /** Results from each webhook endpoint */
  results: Map<string, WebhookDeliveryResult>
  /** Error message if job failed */
  error?: string
}

/**
 * Configuration for the webhook dispatcher
 */
export interface WebhookDispatcherConfig {
  /** Redis connection string or Redis instance */
  redis: string | Redis
  /** Queue name for webhook jobs */
  queueName: string
  /** Default job options */
  defaultJobOptions: {
    attempts: number
    delay: number
    backoff: 'fixed' | 'exponential'
    removeOnComplete: number
    removeOnFail: number
  }
  /** Maximum concurrent webhook jobs */
  concurrency: number
  /** Whether to enable job progress tracking */
  enableProgressTracking: boolean
}

/**
 * Server-side webhook dispatcher with queue-based delivery and status tracking
 */
export class WebhookDispatcher {
  private webhookQueue: Queue.Queue<WebhookJob>
  private deliveryStatuses: Map<string, WebhookDeliveryStatus> = new Map()
  private webhookManager: WebhookManager
  private redis: Redis

  constructor(private config: WebhookDispatcherConfig) {
    // Setup Redis connection
    this.redis =
      typeof config.redis === 'string' ? new Redis(config.redis) : config.redis

    // Create webhook queue
    this.webhookQueue = new Queue(config.queueName, {
      redis: this.redis as any,
      defaultJobOptions: {
        attempts: config.defaultJobOptions.attempts,
        delay: config.defaultJobOptions.delay,
        backoff: {
          type: config.defaultJobOptions.backoff,
          delay: config.defaultJobOptions.delay,
        },
        removeOnComplete: config.defaultJobOptions.removeOnComplete,
        removeOnFail: config.defaultJobOptions.removeOnFail,
      },
    })

    // Create webhook manager
    this.webhookManager = createWebhookManager()

    // Setup queue processing
    this.setupQueueProcessing()

    log.info('WebhookDispatcher initialized', {
      queueName: config.queueName,
      concurrency: config.concurrency,
      redisConnected: this.redis.status === 'ready',
    })
  }

  /**
   * Setup queue processing and event handlers
   */
  private setupQueueProcessing(): void {
    // Process webhook jobs
    this.webhookQueue.process(
      this.config.concurrency,
      this.processWebhookJob.bind(this)
    )

    // Setup event handlers for job status tracking
    this.webhookQueue.on('active', (job) => {
      this.updateDeliveryStatus(job.data.id, {
        status: 'processing',
        processedAt: new Date(),
      })
    })

    this.webhookQueue.on('completed', (job, result) => {
      this.updateDeliveryStatus(job.data.id, {
        status: 'completed',
        completedAt: new Date(),
        results: new Map(Object.entries(result.results)),
      })

      log.info('Webhook job completed', {
        jobId: job.data.id,
        runId: job.data.runId,
        event: job.data.payload.event,
      })
    })

    this.webhookQueue.on('failed', (job, err) => {
      this.updateDeliveryStatus(job.data.id, {
        status: 'failed',
        completedAt: new Date(),
        error: err.message,
      })

      log.error('Webhook job failed', {
        jobId: job.data.id,
        runId: job.data.runId,
        event: job.data.payload.event,
        error: err.message,
      })
    })

    this.webhookQueue.on('progress', (job, progress) => {
      if (this.config.enableProgressTracking) {
        log.debug('Webhook job progress', {
          jobId: job.data.id,
          progress,
        })
      }
    })

    // Handle queue errors
    this.webhookQueue.on('error', (error) => {
      log.error('Webhook queue error', { error })
    })
  }

  /**
   * Process a webhook job
   */
  private async processWebhookJob(job: Queue.Job<WebhookJob>): Promise<{
    success: boolean
    results: Record<string, WebhookDeliveryResult>
  }> {
    const { id, runId, payload, webhookConfigs, useOnlyProvidedConfigs } =
      job.data

    try {
      // Create a temporary webhook manager for this job
      const jobWebhookManager = createWebhookManager(runId)

      // Register webhook configurations
      if (webhookConfigs && useOnlyProvidedConfigs) {
        // Use only the provided configurations
        for (const [webhookId, config] of Object.entries(webhookConfigs)) {
          jobWebhookManager.registerWebhook(webhookId, config)
        }
      } else {
        // Use registered webhooks, optionally adding provided configs
        const existingConfigs = this.webhookManager.getWebhookStatus()
        for (const { id: _webhookId } of existingConfigs) {
          // Copy existing webhooks to job manager
          // Note: We'd need to expose the full config from WebhookManager to do this properly
          // For now, we'll use the main webhook manager
        }

        if (webhookConfigs) {
          for (const [webhookId, config] of Object.entries(webhookConfigs)) {
            jobWebhookManager.registerWebhook(webhookId, config)
          }
        }
      }

      // Update job progress
      if (this.config.enableProgressTracking) {
        await job.progress(10)
      }

      // Send webhooks
      const results = useOnlyProvidedConfigs
        ? await jobWebhookManager.sendWebhook(payload)
        : await this.webhookManager.sendWebhook(payload)

      // Update job progress
      if (this.config.enableProgressTracking) {
        await job.progress(90)
      }

      // Convert Map to Record for serialization
      const resultRecord: Record<string, WebhookDeliveryResult> = {}
      for (const [key, value] of results.entries()) {
        resultRecord[key] = value
      }

      // Update attempts count
      this.updateDeliveryStatus(id, {
        attempts: job.attemptsMade,
      })

      const successCount = Array.from(results.values()).filter(
        (r) => r.success
      ).length
      const totalCount = results.size

      log.info('Webhook job processed', {
        jobId: id,
        runId,
        event: payload.event,
        successCount,
        totalCount,
        attempts: job.attemptsMade,
      })

      if (this.config.enableProgressTracking) {
        await job.progress(100)
      }

      return {
        success: successCount === totalCount,
        results: resultRecord,
      }
    } catch (error) {
      log.error('Failed to process webhook job', {
        jobId: id,
        runId,
        event: payload.event,
        error: error instanceof Error ? error.message : String(error),
      })
      throw error
    }
  }

  /**
   * Dispatch a webhook payload
   */
  public async dispatch(
    payload: WebhookPayload,
    options: {
      /** Run ID associated with this webhook */
      runId?: string
      /** Custom webhook configurations for this dispatch */
      webhookConfigs?: Record<string, WebhookConfig>
      /** Whether to use only provided configs (ignore registered webhooks) */
      useOnlyProvidedConfigs?: boolean
      /** Job priority (higher number = higher priority) */
      priority?: number
      /** Custom delay before processing */
      delay?: number
      /** Custom retry configuration */
      retryConfig?: {
        attempts: number
        delay: number
        backoff: 'fixed' | 'exponential'
      }
    } = {}
  ): Promise<string> {
    const jobId = `webhook_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`

    const jobData: WebhookJob = {
      id: jobId,
      runId: options.runId,
      payload,
      webhookConfigs: options.webhookConfigs,
      useOnlyProvidedConfigs: options.useOnlyProvidedConfigs,
      retryConfig: options.retryConfig,
    }

    // Create delivery status entry
    this.deliveryStatuses.set(jobId, {
      jobId,
      runId: options.runId,
      event: payload.event,
      status: 'pending',
      createdAt: new Date(),
      attempts: 0,
      results: new Map(),
    })

    // Queue job options
    const jobOptions: Queue.JobOptions = {
      priority: options.priority,
      delay: options.delay,
    }

    if (options.retryConfig) {
      jobOptions.attempts = options.retryConfig.attempts
      jobOptions.backoff = {
        type: options.retryConfig.backoff,
        delay: options.retryConfig.delay,
      }
    }

    // Add job to queue
    const job = await this.webhookQueue.add(jobData, jobOptions)

    log.info('Webhook job queued', {
      jobId,
      runId: options.runId,
      event: payload.event,
      priority: options.priority,
      delay: options.delay,
      queuedJobId: job.id,
    })

    return jobId
  }

  /**
   * Register a webhook configuration
   */
  public registerWebhook(id: string, config: WebhookConfig): void {
    this.webhookManager.registerWebhook(id, config)
  }

  /**
   * Unregister a webhook configuration
   */
  public unregisterWebhook(id: string): void {
    this.webhookManager.unregisterWebhook(id)
  }

  /**
   * Get webhook status
   */
  public getWebhookStatus() {
    return this.webhookManager.getWebhookStatus()
  }

  /**
   * Get delivery status for a webhook job
   */
  public getDeliveryStatus(jobId: string): WebhookDeliveryStatus | undefined {
    return this.deliveryStatuses.get(jobId)
  }

  /**
   * Get all delivery statuses for a run
   */
  public getRunDeliveryStatuses(runId: string): WebhookDeliveryStatus[] {
    return Array.from(this.deliveryStatuses.values()).filter(
      (status) => status.runId === runId
    )
  }

  /**
   * Update delivery status
   */
  private updateDeliveryStatus(
    jobId: string,
    updates: Partial<WebhookDeliveryStatus>
  ): void {
    const existingStatus = this.deliveryStatuses.get(jobId)
    if (existingStatus) {
      Object.assign(existingStatus, updates)
    }
  }

  /**
   * Get queue statistics
   */
  public async getQueueStats() {
    const waiting = await this.webhookQueue.getWaiting()
    const active = await this.webhookQueue.getActive()
    const completed = await this.webhookQueue.getCompleted()
    const failed = await this.webhookQueue.getFailed()
    const delayed = await this.webhookQueue.getDelayed()

    return {
      waiting: waiting.length,
      active: active.length,
      completed: completed.length,
      failed: failed.length,
      delayed: delayed.length,
      total:
        waiting.length +
        active.length +
        completed.length +
        failed.length +
        delayed.length,
    }
  }

  /**
   * Clear old delivery statuses (cleanup)
   */
  public clearOldDeliveryStatuses(
    olderThanMs: number = 24 * 60 * 60 * 1000
  ): number {
    const cutoffTime = new Date(Date.now() - olderThanMs)
    let removedCount = 0

    for (const [jobId, status] of this.deliveryStatuses.entries()) {
      if (status.createdAt < cutoffTime) {
        this.deliveryStatuses.delete(jobId)
        removedCount++
      }
    }

    log.info('Cleared old delivery statuses', { removedCount, cutoffTime })
    return removedCount
  }

  /**
   * Pause the webhook queue
   */
  public async pause(): Promise<void> {
    await this.webhookQueue.pause()
    log.info('Webhook queue paused')
  }

  /**
   * Resume the webhook queue
   */
  public async resume(): Promise<void> {
    await this.webhookQueue.resume()
    log.info('Webhook queue resumed')
  }

  /**
   * Close the dispatcher and clean up resources
   */
  public async close(): Promise<void> {
    await this.webhookQueue.close()
    await this.redis.disconnect()
    this.deliveryStatuses.clear()
    log.info('WebhookDispatcher closed')
  }
}

/**
 * Create a new webhook dispatcher instance
 */
export function createWebhookDispatcher(
  config: WebhookDispatcherConfig
): WebhookDispatcher {
  return new WebhookDispatcher(config)
}

/**
 * Default webhook dispatcher configuration
 */
export const defaultWebhookDispatcherConfig: Partial<WebhookDispatcherConfig> =
  {
    queueName: 'scrapix-webhooks',
    defaultJobOptions: {
      attempts: 3,
      delay: 1000,
      backoff: 'exponential',
      removeOnComplete: 100,
      removeOnFail: 50,
    },
    concurrency: 5,
    enableProgressTracking: false,
  }
