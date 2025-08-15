import { EventEmitter } from 'events'
import { Config, DocumentType, CrawlerType } from '../types'

/**
 * Event data interfaces for different crawler events
 */
export interface PageCrawledEvent {
  /** The URL that was crawled */
  url: string
  /** Timestamp when the page was crawled */
  timestamp: Date
  /** Success status of the crawl */
  success: boolean
  /** Time taken to crawl the page in milliseconds */
  duration?: number
  /** Memory usage after crawling the page */
  memoryUsage?: NodeJS.MemoryUsage
  /** Depth of the page in the crawl tree */
  depth?: number
  /** Crawler type used */
  crawlerType: CrawlerType
  /** Error message if crawling failed */
  error?: string
  /** Total pages crawled so far */
  totalCrawled: number
  /** Total pages indexed so far */
  totalIndexed: number
}

export interface PageIndexedEvent {
  /** The URL that was indexed */
  url: string
  /** Timestamp when the page was indexed */
  timestamp: Date
  /** Success status of the indexing */
  success: boolean
  /** Time taken to process the page in milliseconds */
  duration?: number
  /** Document that was created */
  document?: DocumentType
  /** Number of documents created from this page */
  documentCount: number
  /** Features that were applied to this page */
  featuresApplied: string[]
  /** Error message if indexing failed */
  error?: string
  /** Total pages indexed so far */
  totalIndexed: number
}

export interface PageErrorEvent {
  /** The URL that encountered an error */
  url: string
  /** Timestamp when the error occurred */
  timestamp: Date
  /** The error that occurred */
  error: Error | string
  /** Stage where the error occurred (crawling, indexing, processing) */
  stage: 'crawling' | 'indexing' | 'processing'
  /** Whether this was a retry attempt */
  isRetry?: boolean
  /** Retry attempt number if applicable */
  retryAttempt?: number
  /** Crawler type used */
  crawlerType: CrawlerType
  /** Additional context about the error */
  context?: Record<string, any>
}

export interface BatchSentEvent {
  /** Timestamp when the batch was sent */
  timestamp: Date
  /** Number of documents in the batch */
  documentCount: number
  /** Success status of the batch operation */
  success: boolean
  /** Time taken to send the batch in milliseconds */
  duration?: number
  /** Meilisearch index where documents were sent */
  indexUid: string
  /** Task ID returned by Meilisearch */
  taskId?: number
  /** Size of the batch in bytes (approximate) */
  batchSizeBytes?: number
  /** Error message if batch sending failed */
  error?: string
  /** Total documents sent so far */
  totalDocumentsSent: number
  /** Whether this was a retry attempt */
  isRetry?: boolean
  /** Retry attempt number if applicable */
  retryAttempt?: number
}

export interface CrawlerStartedEvent {
  /** Timestamp when the crawler started */
  timestamp: Date
  /** Configuration used for crawling */
  config: Config
  /** Starting URLs */
  startUrls: string[]
  /** Crawler type being used */
  crawlerType: CrawlerType
  /** Features enabled */
  featuresEnabled: string[]
  /** Target Meilisearch index */
  indexUid: string
  /** Estimated total URLs to crawl (if available) */
  estimatedTotal?: number
}

export interface CrawlerCompletedEvent {
  /** Timestamp when the crawler completed */
  timestamp: Date
  /** Total time taken for the crawl in milliseconds */
  duration: number
  /** Total pages crawled */
  totalCrawled: number
  /** Total pages indexed */
  totalIndexed: number
  /** Total documents sent to Meilisearch */
  totalDocumentsSent: number
  /** Whether the crawl was successful */
  success: boolean
  /** Final memory usage */
  memoryUsage?: NodeJS.MemoryUsage
  /** Error message if the crawl failed */
  error?: string
  /** Statistics about the crawl */
  stats: {
    /** Average time per page in milliseconds */
    avgPageTime: number
    /** Pages per minute */
    pagesPerMinute: number
    /** Success rate as a percentage */
    successRate: number
    /** Number of retries */
    totalRetries: number
    /** Number of errors encountered */
    totalErrors: number
  }
}

export interface CrawlerPausedEvent {
  /** Timestamp when the crawler was paused */
  timestamp: Date
  /** Reason for pausing */
  reason: string
  /** Current progress */
  progress: {
    crawled: number
    indexed: number
    documentsSent: number
  }
}

export interface CrawlerResumedEvent {
  /** Timestamp when the crawler was resumed */
  timestamp: Date
  /** Duration of the pause in milliseconds */
  pauseDuration: number
  /** Current progress */
  progress: {
    crawled: number
    indexed: number
    documentsSent: number
  }
}

export interface ProgressUpdateEvent {
  /** Timestamp of the progress update */
  timestamp: Date
  /** Current progress counts */
  progress: {
    crawled: number
    indexed: number
    documentsSent: number
    errors: number
  }
  /** Estimated completion percentage (0-100) */
  completionPercentage?: number
  /** Estimated time remaining in milliseconds */
  estimatedTimeRemaining?: number
  /** Current crawling rate (pages per minute) */
  currentRate: number
  /** Average time per page in milliseconds */
  avgPageTime: number
  /** Memory usage */
  memoryUsage?: NodeJS.MemoryUsage
}

/**
 * Event names as string literals for type safety
 */
export const CrawlerEvents = {
  PAGE_CRAWLED: 'page.crawled',
  PAGE_INDEXED: 'page.indexed',
  PAGE_ERROR: 'page.error',
  BATCH_SENT: 'batch.sent',
  CRAWLER_STARTED: 'crawler.started',
  CRAWLER_COMPLETED: 'crawler.completed',
  CRAWLER_PAUSED: 'crawler.paused',
  CRAWLER_RESUMED: 'crawler.resumed',
  PROGRESS_UPDATE: 'progress.update',
} as const

export type CrawlerEventName =
  (typeof CrawlerEvents)[keyof typeof CrawlerEvents]

/**
 * Event type mapping for type-safe event handling
 */
export interface CrawlerEventMap {
  [CrawlerEvents.PAGE_CRAWLED]: PageCrawledEvent
  [CrawlerEvents.PAGE_INDEXED]: PageIndexedEvent
  [CrawlerEvents.PAGE_ERROR]: PageErrorEvent
  [CrawlerEvents.BATCH_SENT]: BatchSentEvent
  [CrawlerEvents.CRAWLER_STARTED]: CrawlerStartedEvent
  [CrawlerEvents.CRAWLER_COMPLETED]: CrawlerCompletedEvent
  [CrawlerEvents.CRAWLER_PAUSED]: CrawlerPausedEvent
  [CrawlerEvents.CRAWLER_RESUMED]: CrawlerResumedEvent
  [CrawlerEvents.PROGRESS_UPDATE]: ProgressUpdateEvent
}

/**
 * Enhanced EventEmitter for crawler events with type safety and built-in progress tracking
 */
export class CrawlerEventEmitter extends EventEmitter {
  private startTime?: Date
  private lastProgressUpdate?: Date
  private progressUpdateInterval: number
  private progressTimer?: NodeJS.Timeout

  constructor(progressUpdateIntervalMs: number = 5000) {
    super()
    this.progressUpdateInterval = progressUpdateIntervalMs
    this.setMaxListeners(100) // Allow many listeners for monitoring
  }

  /**
   * Type-safe event emission
   */
  emit<K extends keyof CrawlerEventMap>(
    eventName: K,
    eventData: CrawlerEventMap[K]
  ): boolean {
    return super.emit(eventName, eventData)
  }

  /**
   * Type-safe event listening
   */
  on<K extends keyof CrawlerEventMap>(
    eventName: K,
    listener: (eventData: CrawlerEventMap[K]) => void
  ): this {
    return super.on(eventName, listener)
  }

  /**
   * Type-safe one-time event listening
   */
  once<K extends keyof CrawlerEventMap>(
    eventName: K,
    listener: (eventData: CrawlerEventMap[K]) => void
  ): this {
    return super.once(eventName, listener)
  }

  /**
   * Type-safe event listener removal
   */
  off<K extends keyof CrawlerEventMap>(
    eventName: K,
    listener: (eventData: CrawlerEventMap[K]) => void
  ): this {
    return super.off(eventName, listener)
  }

  /**
   * Emit a page crawled event
   */
  emitPageCrawled(data: Omit<PageCrawledEvent, 'timestamp'>) {
    this.emit(CrawlerEvents.PAGE_CRAWLED, {
      ...data,
      timestamp: new Date(),
    })
  }

  /**
   * Emit a page indexed event
   */
  emitPageIndexed(data: Omit<PageIndexedEvent, 'timestamp'>) {
    this.emit(CrawlerEvents.PAGE_INDEXED, {
      ...data,
      timestamp: new Date(),
    })
  }

  /**
   * Emit a page error event
   */
  emitPageError(data: Omit<PageErrorEvent, 'timestamp'>) {
    this.emit(CrawlerEvents.PAGE_ERROR, {
      ...data,
      timestamp: new Date(),
    })
  }

  /**
   * Emit a batch sent event
   */
  emitBatchSent(data: Omit<BatchSentEvent, 'timestamp'>) {
    this.emit(CrawlerEvents.BATCH_SENT, {
      ...data,
      timestamp: new Date(),
    })
  }

  /**
   * Emit a crawler started event and begin progress tracking
   */
  emitCrawlerStarted(data: Omit<CrawlerStartedEvent, 'timestamp'>) {
    this.startTime = new Date()
    this.lastProgressUpdate = this.startTime

    this.emit(CrawlerEvents.CRAWLER_STARTED, {
      ...data,
      timestamp: this.startTime,
    })

    this.startProgressTracking()
  }

  /**
   * Emit a crawler completed event and stop progress tracking
   */
  emitCrawlerCompleted(
    data: Omit<CrawlerCompletedEvent, 'timestamp' | 'duration'>
  ) {
    this.stopProgressTracking()

    const endTime = new Date()
    const duration = this.startTime
      ? endTime.getTime() - this.startTime.getTime()
      : 0

    this.emit(CrawlerEvents.CRAWLER_COMPLETED, {
      ...data,
      timestamp: endTime,
      duration,
    })
  }

  /**
   * Emit a crawler paused event and stop progress tracking
   */
  emitCrawlerPaused(data: Omit<CrawlerPausedEvent, 'timestamp'>) {
    this.stopProgressTracking()

    this.emit(CrawlerEvents.CRAWLER_PAUSED, {
      ...data,
      timestamp: new Date(),
    })
  }

  /**
   * Emit a crawler resumed event and restart progress tracking
   */
  emitCrawlerResumed(data: Omit<CrawlerResumedEvent, 'timestamp'>) {
    this.emit(CrawlerEvents.CRAWLER_RESUMED, {
      ...data,
      timestamp: new Date(),
    })

    this.startProgressTracking()
  }

  /**
   * Emit a progress update event
   */
  emitProgressUpdate(data: Omit<ProgressUpdateEvent, 'timestamp'>) {
    this.lastProgressUpdate = new Date()

    this.emit(CrawlerEvents.PROGRESS_UPDATE, {
      ...data,
      timestamp: this.lastProgressUpdate,
    })
  }

  /**
   * Start automatic progress tracking
   */
  private startProgressTracking() {
    if (this.progressTimer) {
      clearInterval(this.progressTimer)
    }

    // Note: Progress updates need to be triggered by the crawler
    // This just sets up the interval - actual data comes from crawler
    this.progressTimer = setInterval(() => {
      // This is a placeholder - actual progress updates should be triggered
      // by the crawler when it has updated data
    }, this.progressUpdateInterval)
  }

  /**
   * Stop automatic progress tracking
   */
  private stopProgressTracking() {
    if (this.progressTimer) {
      clearInterval(this.progressTimer)
      this.progressTimer = undefined
    }
  }

  /**
   * Get elapsed time since crawler started
   */
  getElapsedTime(): number {
    return this.startTime ? new Date().getTime() - this.startTime.getTime() : 0
  }

  /**
   * Calculate current crawling rate
   */
  calculateRate(totalProcessed: number): number {
    const elapsedMinutes = this.getElapsedTime() / (1000 * 60)
    return elapsedMinutes > 0 ? totalProcessed / elapsedMinutes : 0
  }

  /**
   * Calculate estimated time remaining
   */
  estimateTimeRemaining(progress: number, total: number): number {
    if (progress === 0 || total === 0) return 0

    const elapsedTime = this.getElapsedTime()
    const progressRate = progress / elapsedTime
    const remaining = total - progress

    return progressRate > 0 ? remaining / progressRate : 0
  }

  /**
   * Clean up resources
   */
  destroy() {
    this.stopProgressTracking()
    this.removeAllListeners()
  }
}

/**
 * Utility function to create a new crawler event emitter
 */
export function createCrawlerEventEmitter(
  progressUpdateIntervalMs?: number
): CrawlerEventEmitter {
  return new CrawlerEventEmitter(progressUpdateIntervalMs)
}

/**
 * Type guard to check if an event is a specific crawler event
 */
export function isCrawlerEvent<K extends keyof CrawlerEventMap>(
  eventName: string,
  eventData: any,
  targetEventName: K
): eventData is CrawlerEventMap[K] {
  return (
    eventName === targetEventName &&
    typeof eventData === 'object' &&
    eventData !== null
  )
}
