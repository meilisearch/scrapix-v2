import {
  CrawlerEventEmitter,
  CrawlerEventMap,
  CrawlerEvents,
} from './crawler-events'
import { Config } from '../types'

/**
 * Metrics aggregation interface for tracking crawler performance
 */
export interface CrawlerMetrics {
  /** Total number of pages crawled */
  totalCrawled: number
  /** Total number of pages indexed */
  totalIndexed: number
  /** Total number of documents sent to Meilisearch */
  totalDocumentsSent: number
  /** Total number of errors encountered */
  totalErrors: number
  /** Total number of retries performed */
  totalRetries: number
  /** Start time of the crawl */
  startTime?: Date
  /** End time of the crawl */
  endTime?: Date
  /** Current crawl rate (pages per minute) */
  currentRate: number
  /** Average processing time per page */
  avgPageTime: number
  /** Memory usage statistics */
  memoryUsage?: NodeJS.MemoryUsage
  /** Success rate percentage (0-100) */
  successRate: number
  /** Estimated completion percentage */
  completionPercentage?: number
  /** Estimated time remaining in milliseconds */
  estimatedTimeRemaining?: number
}

/**
 * Event aggregation data for batch processing
 */
export interface EventBatch {
  /** Batch identifier */
  id: string
  /** Events in this batch */
  events: Array<{
    eventName: keyof CrawlerEventMap
    eventData: any
    timestamp: Date
  }>
  /** Batch creation timestamp */
  createdAt: Date
  /** Whether this batch has been processed */
  processed: boolean
}

/**
 * Singleton EventBus class that extends CrawlerEventEmitter
 * Provides centralized event management with metrics tracking and aggregation
 */
export class EventBus extends CrawlerEventEmitter {
  private static instance: EventBus | null = null
  private metrics: CrawlerMetrics
  private config?: Config
  private eventBatches: Map<string, EventBatch> = new Map()
  private batchSize: number = 50
  private currentBatch: EventBatch | null = null
  private batchTimeout: NodeJS.Timeout | null = null
  private readonly BATCH_TIMEOUT_MS = 5000

  private constructor() {
    super()
    this.metrics = this.initializeMetrics()
    this.setupMetricTracking()
  }

  /**
   * Get the singleton instance of EventBus
   */
  public static getInstance(): EventBus {
    if (!EventBus.instance) {
      EventBus.instance = new EventBus()
    }
    return EventBus.instance
  }

  /**
   * Initialize the EventBus with configuration
   */
  public initialize(config: Config): void {
    this.config = config
    this.metrics = this.initializeMetrics()
  }

  /**
   * Reset the singleton instance (useful for testing)
   */
  public static reset(): void {
    if (EventBus.instance) {
      EventBus.instance.destroy()
      EventBus.instance = null
    }
  }

  /**
   * Initialize metrics with default values
   */
  private initializeMetrics(): CrawlerMetrics {
    return {
      totalCrawled: 0,
      totalIndexed: 0,
      totalDocumentsSent: 0,
      totalErrors: 0,
      totalRetries: 0,
      currentRate: 0,
      avgPageTime: 0,
      successRate: 100,
      completionPercentage: 0,
      estimatedTimeRemaining: 0,
    }
  }

  /**
   * Set up automatic metric tracking based on events
   */
  private setupMetricTracking(): void {
    // Track crawler started
    this.on(CrawlerEvents.CRAWLER_STARTED, (data) => {
      this.metrics.startTime = data.timestamp
    })

    // Track crawler completed
    this.on(CrawlerEvents.CRAWLER_COMPLETED, (data) => {
      this.metrics.endTime = data.timestamp
      this.metrics.totalCrawled = data.totalCrawled
      this.metrics.totalIndexed = data.totalIndexed
      this.metrics.totalDocumentsSent = data.totalDocumentsSent
      this.metrics.successRate = data.stats.successRate
      this.metrics.avgPageTime = data.stats.avgPageTime
      this.metrics.totalRetries = data.stats.totalRetries
      this.metrics.totalErrors = data.stats.totalErrors
      this.metrics.memoryUsage = data.memoryUsage
    })

    // Track page crawled
    this.on(CrawlerEvents.PAGE_CRAWLED, (data) => {
      this.metrics.totalCrawled = data.totalCrawled
      if (!data.success) {
        this.metrics.totalErrors++
      }
      if (data.memoryUsage) {
        this.metrics.memoryUsage = data.memoryUsage
      }
    })

    // Track page indexed
    this.on(CrawlerEvents.PAGE_INDEXED, (data) => {
      this.metrics.totalIndexed = data.totalIndexed
      if (!data.success) {
        this.metrics.totalErrors++
      }
    })

    // Track batch sent
    this.on(CrawlerEvents.BATCH_SENT, (data) => {
      this.metrics.totalDocumentsSent = data.totalDocumentsSent
      if (!data.success) {
        this.metrics.totalErrors++
      }
      if (data.isRetry) {
        this.metrics.totalRetries++
      }
    })

    // Track errors
    this.on(CrawlerEvents.PAGE_ERROR, (data) => {
      this.metrics.totalErrors++
      if (data.isRetry) {
        this.metrics.totalRetries++
      }
    })

    // Track progress updates
    this.on(CrawlerEvents.PROGRESS_UPDATE, (data) => {
      this.metrics.totalCrawled = data.progress.crawled
      this.metrics.totalIndexed = data.progress.indexed
      this.metrics.totalDocumentsSent = data.progress.documentsSent
      this.metrics.totalErrors = data.progress.errors
      this.metrics.currentRate = data.currentRate
      this.metrics.avgPageTime = data.avgPageTime
      this.metrics.completionPercentage = data.completionPercentage
      this.metrics.estimatedTimeRemaining = data.estimatedTimeRemaining
      if (data.memoryUsage) {
        this.metrics.memoryUsage = data.memoryUsage
      }

      // Calculate success rate
      const totalAttempts = this.metrics.totalCrawled
      if (totalAttempts > 0) {
        this.metrics.successRate =
          ((totalAttempts - this.metrics.totalErrors) / totalAttempts) * 100
      }
    })
  }

  /**
   * Get current crawler metrics
   */
  public getMetrics(): CrawlerMetrics {
    return { ...this.metrics }
  }

  /**
   * Reset metrics to initial state
   */
  public resetMetrics(): void {
    this.metrics = this.initializeMetrics()
  }

  /**
   * Get crawler configuration
   */
  public getConfig(): Config | undefined {
    return this.config
  }

  /**
   * Override emit to also track events in batches
   */
  public emit<K extends keyof CrawlerEventMap>(
    eventName: K,
    eventData: CrawlerEventMap[K]
  ): boolean {
    // Add event to current batch for potential forwarding
    this.addEventToBatch(eventName, eventData)

    // Call parent emit
    return super.emit(eventName, eventData)
  }

  /**
   * Add an event to the current batch for processing
   */
  private addEventToBatch<K extends keyof CrawlerEventMap>(
    eventName: K,
    eventData: CrawlerEventMap[K]
  ): void {
    if (!this.currentBatch) {
      this.currentBatch = {
        id: this.generateBatchId(),
        events: [],
        createdAt: new Date(),
        processed: false,
      }
    }

    this.currentBatch.events.push({
      eventName,
      eventData,
      timestamp: new Date(),
    })

    // Process batch if it's full
    if (this.currentBatch.events.length >= this.batchSize) {
      this.processBatch()
    } else {
      // Set up timeout to process batch
      this.schedulesBatchProcessing()
    }
  }

  /**
   * Schedule batch processing after timeout
   */
  private schedulesBatchProcessing(): void {
    if (this.batchTimeout) {
      clearTimeout(this.batchTimeout)
    }

    this.batchTimeout = setTimeout(() => {
      this.processBatch()
    }, this.BATCH_TIMEOUT_MS)
  }

  /**
   * Process the current batch
   */
  private processBatch(): void {
    if (!this.currentBatch || this.currentBatch.events.length === 0) {
      return
    }

    // Store the batch
    this.eventBatches.set(this.currentBatch.id, { ...this.currentBatch })

    // Emit batch processed event for listeners (like IPC forwarders)
    super.emit('batch.processed' as any, {
      batchId: this.currentBatch.id,
      events: [...this.currentBatch.events],
      timestamp: new Date(),
    })

    // Clear current batch
    this.currentBatch = null

    // Clear timeout
    if (this.batchTimeout) {
      clearTimeout(this.batchTimeout)
      this.batchTimeout = null
    }

    // Clean up old batches (keep only last 100)
    this.cleanupOldBatches()
  }

  /**
   * Clean up old event batches to prevent memory leaks
   */
  private cleanupOldBatches(): void {
    const MAX_BATCHES = 100
    if (this.eventBatches.size > MAX_BATCHES) {
      const batches = Array.from(this.eventBatches.entries()).sort(
        ([, a], [, b]) => a.createdAt.getTime() - b.createdAt.getTime()
      )

      // Remove oldest batches
      const toRemove = batches.slice(0, batches.length - MAX_BATCHES)
      toRemove.forEach(([batchId]) => {
        this.eventBatches.delete(batchId)
      })
    }
  }

  /**
   * Generate a unique batch ID
   */
  private generateBatchId(): string {
    return `batch_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`
  }

  /**
   * Get all event batches
   */
  public getEventBatches(): EventBatch[] {
    return Array.from(this.eventBatches.values())
  }

  /**
   * Get a specific event batch by ID
   */
  public getEventBatch(batchId: string): EventBatch | undefined {
    return this.eventBatches.get(batchId)
  }

  /**
   * Mark a batch as processed
   */
  public markBatchProcessed(batchId: string): void {
    const batch = this.eventBatches.get(batchId)
    if (batch) {
      batch.processed = true
    }
  }

  /**
   * Force processing of current batch
   */
  public flushCurrentBatch(): void {
    if (this.currentBatch && this.currentBatch.events.length > 0) {
      this.processBatch()
    }
  }

  /**
   * Get summary statistics for monitoring
   */
  public getSummaryStats(): {
    metrics: CrawlerMetrics
    eventBatches: {
      total: number
      processed: number
      pending: number
    }
    runtime: {
      elapsedTime: number
      estimatedTotal?: number
    }
  } {
    const batches = this.getEventBatches()
    const processed = batches.filter((b) => b.processed).length
    const pending = batches.length - processed

    return {
      metrics: this.getMetrics(),
      eventBatches: {
        total: batches.length,
        processed,
        pending,
      },
      runtime: {
        elapsedTime: this.getElapsedTime(),
        estimatedTotal: this.metrics.estimatedTimeRemaining
          ? this.getElapsedTime() + this.metrics.estimatedTimeRemaining
          : undefined,
      },
    }
  }

  /**
   * Enhanced destroy method to clean up all resources
   */
  public destroy(): void {
    // Clear batch timeout
    if (this.batchTimeout) {
      clearTimeout(this.batchTimeout)
      this.batchTimeout = null
    }

    // Process any remaining events
    this.flushCurrentBatch()

    // Clear event batches
    this.eventBatches.clear()
    this.currentBatch = null

    // Reset metrics
    this.resetMetrics()

    // Call parent destroy
    super.destroy()
  }
}

/**
 * Convenience function to get the EventBus singleton
 */
export function getEventBus(): EventBus {
  return EventBus.getInstance()
}

/**
 * Convenience function to initialize EventBus with configuration
 */
export function initializeEventBus(config: Config): EventBus {
  const eventBus = EventBus.getInstance()
  eventBus.initialize(config)
  return eventBus
}

/**
 * Type definitions for batch processed event
 */
export interface BatchProcessedEvent {
  batchId: string
  events: Array<{
    eventName: keyof CrawlerEventMap
    eventData: any
    timestamp: Date
  }>
  timestamp: Date
}

// Types are already exported above
