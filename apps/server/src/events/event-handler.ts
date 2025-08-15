import { EventEmitter } from 'events'
import { Log } from 'crawlee'
import {
  CrawlerEventMap,
  BatchProcessedEvent,
  EventStorage,
} from '@scrapix/core'
import { Response } from 'express'

const log = new Log({ prefix: 'EventHandler' })

/**
 * SSE client interface for tracking connected clients
 */
export interface SSEClient {
  id: string
  jobId: string
  response: Response
  connectedAt: Date
  lastPing?: Date
}

/**
 * Supabase event interface (stubbed for now)
 */
export interface SupabaseEventData {
  eventType: string
  eventData: any
  timestamp: Date
  jobId: string
  metadata?: Record<string, any>
}

/**
 * Event handler for managing crawler events and forwarding them to clients
 * Handles real-time event streaming via Server-Sent Events (SSE) and
 * can forward events to external systems like Supabase
 */
export class EventHandler extends EventEmitter {
  private sseClients: Map<string, SSEClient> = new Map()
  private jobClients: Map<string, Set<string>> = new Map() // jobId -> Set of clientIds
  private supabaseEnabled: boolean = false
  private eventStorage?: EventStorage
  private keepAliveInterval?: NodeJS.Timeout
  private readonly KEEP_ALIVE_INTERVAL = 30000 // 30 seconds

  constructor() {
    super()
    this.setMaxListeners(100)
    this.setupKeepAlive()
    this.initializeEventStorage()
  }

  /**
   * Initialize EventStorage if Supabase is configured
   */
  private initializeEventStorage(): void {
    try {
      // Check if Supabase is configured via environment variables
      if (process.env.SUPABASE_URL && process.env.SUPABASE_ANON_KEY) {
        this.eventStorage = new EventStorage()
        this.supabaseEnabled = true
        log.info('EventStorage initialized successfully')
      } else {
        log.info('Supabase not configured, event storage disabled')
      }
    } catch (error) {
      log.warning('Failed to initialize EventStorage', { error })
      this.supabaseEnabled = false
    }
  }

  /**
   * Handle events from crawler process via IPC
   */
  public handleCrawlerEvent<K extends keyof CrawlerEventMap>(
    eventName: K,
    eventData: CrawlerEventMap[K],
    jobId: string
  ): void {
    log.debug('Handling crawler event', { eventName, jobId })

    // Forward to SSE clients
    this.forwardEventToSSE(eventName, eventData, jobId)

    // Forward to Supabase if enabled
    if (this.supabaseEnabled && this.eventStorage) {
      this.persistEventToSupabase(eventName, eventData, jobId)
    }

    // Emit internally for other listeners
    this.emit('crawler.event', {
      eventName,
      eventData,
      jobId,
      timestamp: new Date(),
    })
  }

  /**
   * Handle batch processed events from EventBus
   */
  public handleBatchProcessed(
    batchData: BatchProcessedEvent,
    jobId: string
  ): void {
    log.debug('Handling batch processed event', {
      batchId: batchData.batchId,
      eventCount: batchData.events.length,
      jobId,
    })

    // Forward each event in the batch to SSE clients
    batchData.events.forEach((event) => {
      this.forwardEventToSSE(
        event.eventName,
        event.eventData,
        jobId,
        event.timestamp
      )
    })

    // Also forward to Supabase if enabled
    if (this.supabaseEnabled && this.eventStorage) {
      this.persistBatchToSupabase(batchData, jobId)
    }
  }

  /**
   * Register a new SSE client for real-time event streaming
   */
  public registerSSEClient(
    clientId: string,
    jobId: string,
    response: Response
  ): void {
    log.info('Registering SSE client', { clientId, jobId })

    // Set up SSE headers
    response.writeHead(200, {
      'Content-Type': 'text/event-stream',
      'Cache-Control': 'no-cache',
      Connection: 'keep-alive',
      'Access-Control-Allow-Origin': '*',
      'Access-Control-Allow-Headers': 'Cache-Control',
    })

    // Create client record
    const client: SSEClient = {
      id: clientId,
      jobId,
      response,
      connectedAt: new Date(),
    }

    // Store client
    this.sseClients.set(clientId, client)

    // Track clients by job
    if (!this.jobClients.has(jobId)) {
      this.jobClients.set(jobId, new Set())
    }
    this.jobClients.get(jobId)!.add(clientId)

    // Send initial connection event
    this.sendSSEEvent(clientId, 'connected', {
      message: 'Connected to event stream',
      jobId,
      timestamp: new Date().toISOString(),
    })

    // Handle client disconnect
    response.on('close', () => {
      this.unregisterSSEClient(clientId)
    })

    response.on('error', (error) => {
      log.error('SSE client error', { clientId, error })
      this.unregisterSSEClient(clientId)
    })
  }

  /**
   * Unregister an SSE client
   */
  public unregisterSSEClient(clientId: string): void {
    const client = this.sseClients.get(clientId)
    if (!client) return

    log.info('Unregistering SSE client', { clientId, jobId: client.jobId })

    // Remove from job tracking
    const jobClients = this.jobClients.get(client.jobId)
    if (jobClients) {
      jobClients.delete(clientId)
      if (jobClients.size === 0) {
        this.jobClients.delete(client.jobId)
      }
    }

    // Remove client
    this.sseClients.delete(clientId)

    // End response if still open
    try {
      if (!client.response.headersSent) {
        client.response.end()
      }
    } catch (error) {
      // Ignore errors when closing response
    }
  }

  /**
   * Forward crawler event to SSE clients
   */
  private forwardEventToSSE<K extends keyof CrawlerEventMap>(
    eventName: K,
    eventData: CrawlerEventMap[K],
    jobId: string,
    timestamp?: Date
  ): void {
    const clientIds = this.jobClients.get(jobId)
    if (!clientIds || clientIds.size === 0) {
      return
    }

    const eventPayload = {
      type: eventName,
      data: eventData,
      timestamp: timestamp?.toISOString() || new Date().toISOString(),
      jobId,
    }

    // Send to all clients for this job
    clientIds.forEach((clientId) => {
      this.sendSSEEvent(clientId, 'crawler.event', eventPayload)
    })
  }

  /**
   * Send SSE event to a specific client
   */
  private sendSSEEvent(clientId: string, eventType: string, data: any): void {
    const client = this.sseClients.get(clientId)
    if (!client) return

    try {
      const message = `event: ${eventType}\ndata: ${JSON.stringify(data)}\n\n`
      client.response.write(message)
      client.lastPing = new Date()
    } catch (error) {
      log.error('Error sending SSE event', { clientId, error })
      this.unregisterSSEClient(clientId)
    }
  }

  /**
   * Persist event to Supabase via EventStorage
   */
  private persistEventToSupabase<K extends keyof CrawlerEventMap>(
    eventName: K,
    eventData: CrawlerEventMap[K],
    jobId: string
  ): void {
    if (!this.eventStorage) {
      log.warning('EventStorage not initialized, cannot persist event')
      return
    }

    log.debug('Persisting event to Supabase', { eventName, jobId })

    // Add to batch for efficient storage
    this.eventStorage.addToBatch(jobId, eventName, eventData, {
      source: 'scrapix-crawler',
      version: '1.0.0',
      serverTimestamp: new Date().toISOString(),
    })

    // Also emit for backwards compatibility
    const supabaseEvent: SupabaseEventData = {
      eventType: eventName,
      eventData,
      timestamp: new Date(),
      jobId,
      metadata: {
        source: 'scrapix-crawler',
        version: '1.0.0',
      },
    }

    this.emit('supabase.event', supabaseEvent)
  }

  /**
   * Persist batch of events to Supabase
   */
  private persistBatchToSupabase(
    batchData: BatchProcessedEvent,
    jobId: string
  ): void {
    if (!this.eventStorage) {
      log.warning('EventStorage not initialized, cannot persist batch')
      return
    }

    log.debug('Persisting batch to Supabase', {
      batchId: batchData.batchId,
      eventCount: batchData.events.length,
      jobId,
    })

    // Store batch efficiently
    const eventBatch = {
      events: batchData.events.map((event) => ({
        eventName: event.eventName,
        eventData: event.eventData,
        timestamp: event.timestamp,
        metadata: {
          source: 'scrapix-crawler',
          version: '1.0.0',
          batchId: batchData.batchId,
          serverTimestamp: new Date().toISOString(),
        },
      })),
      runId: jobId,
      batchId: batchData.batchId,
    }

    // Use storeBatch for immediate persistence of large batches
    this.eventStorage.storeBatch(eventBatch).catch((error) => {
      log.error('Failed to store event batch in Supabase', {
        error,
        batchId: batchData.batchId,
      })
    })

    // Also emit for backwards compatibility
    const supabaseBatch = {
      batchId: batchData.batchId,
      events: batchData.events.map((event) => ({
        eventType: event.eventName,
        eventData: event.eventData,
        timestamp: event.timestamp,
        jobId,
      })),
      processedAt: new Date(),
      metadata: {
        source: 'scrapix-crawler',
        version: '1.0.0',
      },
    }

    this.emit('supabase.batch', supabaseBatch)
  }

  /**
   * Enable or disable Supabase integration
   */
  public setSupabaseEnabled(enabled: boolean): void {
    this.supabaseEnabled = enabled
    log.info('Supabase integration', { enabled })
  }

  /**
   * Get statistics about connected clients
   */
  public getClientStats(): {
    totalClients: number
    clientsByJob: Record<string, number>
    oldestConnection: Date | null
  } {
    const clientsByJob: Record<string, number> = {}
    let oldestConnection: Date | null = null

    this.jobClients.forEach((clientIds, jobId) => {
      clientsByJob[jobId] = clientIds.size
    })

    this.sseClients.forEach((client) => {
      if (!oldestConnection || client.connectedAt < oldestConnection) {
        oldestConnection = client.connectedAt
      }
    })

    return {
      totalClients: this.sseClients.size,
      clientsByJob,
      oldestConnection,
    }
  }

  /**
   * Broadcast message to all clients for a specific job
   */
  public broadcastToJob(jobId: string, eventType: string, data: any): void {
    const clientIds = this.jobClients.get(jobId)
    if (!clientIds) return

    clientIds.forEach((clientId) => {
      this.sendSSEEvent(clientId, eventType, data)
    })
  }

  /**
   * Setup keep-alive mechanism to prevent SSE connections from timing out
   */
  private setupKeepAlive(): void {
    this.keepAliveInterval = setInterval(() => {
      const now = new Date()

      // Send ping to all clients
      this.sseClients.forEach((client, clientId) => {
        const timeSinceLastPing = client.lastPing
          ? now.getTime() - client.lastPing.getTime()
          : 0

        // Only ping if it's been more than 25 seconds since last message
        if (timeSinceLastPing > 25000) {
          this.sendSSEEvent(clientId, 'ping', {
            timestamp: now.toISOString(),
          })
        }
      })

      // Clean up stale connections (older than 5 minutes with no activity)
      this.sseClients.forEach((client, clientId) => {
        const timeSinceConnection = now.getTime() - client.connectedAt.getTime()
        const timeSinceLastPing = client.lastPing
          ? now.getTime() - client.lastPing.getTime()
          : timeSinceConnection

        if (timeSinceLastPing > 300000) {
          // 5 minutes
          log.warning('Cleaning up stale SSE connection', {
            clientId,
            jobId: client.jobId,
          })
          this.unregisterSSEClient(clientId)
        }
      })
    }, this.KEEP_ALIVE_INTERVAL)
  }

  /**
   * Cleanup resources
   */
  public async destroy(): Promise<void> {
    log.info('Destroying EventHandler')

    // Clear keep-alive interval
    if (this.keepAliveInterval) {
      clearInterval(this.keepAliveInterval)
    }

    // Clean up EventStorage
    if (this.eventStorage) {
      try {
        await this.eventStorage.destroy()
        log.info('EventStorage cleaned up successfully')
      } catch (error) {
        log.error('Error cleaning up EventStorage', { error })
      }
    }

    // Close all SSE connections
    this.sseClients.forEach((_, clientId) => {
      this.unregisterSSEClient(clientId)
    })

    // Clear all data structures
    this.sseClients.clear()
    this.jobClients.clear()

    // Remove all listeners
    this.removeAllListeners()
  }
}

/**
 * Singleton instance of EventHandler
 */
let eventHandler: EventHandler | null = null

/**
 * Get the singleton EventHandler instance
 */
export function getEventHandler(): EventHandler {
  if (!eventHandler) {
    eventHandler = new EventHandler()
  }
  return eventHandler
}

/**
 * Reset the EventHandler singleton (useful for testing)
 */
export async function resetEventHandler(): Promise<void> {
  if (eventHandler) {
    await eventHandler.destroy()
    eventHandler = null
  }
}

// Types are already exported above
