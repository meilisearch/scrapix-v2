import { v4 as uuidv4 } from 'uuid'
import SupabaseClientManager, { RunEventRow, RunEventInsert } from './client'
import { CrawlerEventMap } from '../events/crawler-events'

/**
 * Interface for event filtering options
 */
export interface EventQueryOptions {
  runId?: string
  eventTypes?: string[]
  limit?: number
  offset?: number
  startDate?: Date
  endDate?: Date
  orderBy?: 'timestamp' | 'event_type'
  ascending?: boolean
}

/**
 * Interface for batch event data
 */
export interface EventBatch {
  events: Array<{
    eventName: string
    eventData: any
    timestamp: Date
    metadata?: Record<string, any>
  }>
  runId: string
  batchId?: string
}

/**
 * Interface for event statistics
 */
export interface EventStats {
  totalEvents: number
  eventsByType: Record<string, number>
  eventsByRunId: Record<string, number>
  dateRange: {
    earliest: Date | null
    latest: Date | null
  }
}

/**
 * EventStorage class handles persistence of crawler events in Supabase
 * Provides efficient batch insertion and querying capabilities
 */
export class EventStorage {
  private client = SupabaseClientManager.getInstance()
  private batchBuffer: Map<string, RunEventInsert[]> = new Map()
  private batchTimeout: NodeJS.Timeout | null = null

  constructor(
    private batchSize = 100,
    private batchTimeoutMs = 5000
  ) {}

  /**
   * Store a single event
   */
  public async storeEvent<K extends keyof CrawlerEventMap>(
    runId: string,
    eventName: K,
    eventData: CrawlerEventMap[K],
    metadata?: Record<string, any>
  ): Promise<string> {
    const eventId = uuidv4()

    const eventRecord: RunEventInsert = {
      id: eventId,
      run_id: runId,
      event_type: eventName,
      event_data: eventData as Record<string, any>,
      timestamp: new Date().toISOString(),
      metadata,
    }

    const { error } = await this.client.from('run_events').insert(eventRecord)

    if (error) {
      throw new Error(`Failed to store event: ${error.message}`)
    }

    return eventId
  }

  /**
   * Add event to batch buffer for efficient bulk insertion
   */
  public addToBatch<K extends keyof CrawlerEventMap>(
    runId: string,
    eventName: K,
    eventData: CrawlerEventMap[K],
    metadata?: Record<string, any>
  ): void {
    const eventRecord: RunEventInsert = {
      id: uuidv4(),
      run_id: runId,
      event_type: eventName,
      event_data: eventData as Record<string, any>,
      timestamp: new Date().toISOString(),
      metadata,
    }

    // Add to buffer
    if (!this.batchBuffer.has(runId)) {
      this.batchBuffer.set(runId, [])
    }
    this.batchBuffer.get(runId)!.push(eventRecord)

    // Check if we need to flush
    const totalBuffered = Array.from(this.batchBuffer.values()).reduce(
      (total, events) => total + events.length,
      0
    )

    if (totalBuffered >= this.batchSize) {
      void this.flushBatch()
    } else {
      this.scheduleBatchFlush()
    }
  }

  /**
   * Store multiple events in a batch
   */
  public async storeBatch(batch: EventBatch): Promise<string[]> {
    const eventRecords: RunEventInsert[] = batch.events.map((event) => ({
      id: uuidv4(),
      run_id: batch.runId,
      event_type: event.eventName,
      event_data: event.eventData,
      timestamp: event.timestamp.toISOString(),
      metadata: event.metadata,
    }))

    const { data, error } = await this.client
      .from('run_events')
      .insert(eventRecords)
      .select('id')

    if (error) {
      throw new Error(`Failed to store event batch: ${error.message}`)
    }

    return data?.map((record) => record.id) || []
  }

  /**
   * Flush the current batch buffer
   */
  public async flushBatch(): Promise<void> {
    if (this.batchBuffer.size === 0) {
      return
    }

    const allEvents: RunEventInsert[] = []
    for (const events of this.batchBuffer.values()) {
      allEvents.push(...events)
    }

    if (allEvents.length === 0) {
      return
    }

    try {
      const { error } = await this.client.from('run_events').insert(allEvents)

      if (error) {
        throw new Error(`Failed to flush event batch: ${error.message}`)
      }

      // Clear the buffer after successful insertion
      this.batchBuffer.clear()

      // Clear the timeout
      if (this.batchTimeout) {
        clearTimeout(this.batchTimeout)
        this.batchTimeout = null
      }
    } catch (error) {
      // Log error but don't throw to prevent blocking the event system
      console.error('Failed to flush event batch:', error)
    }
  }

  /**
   * Schedule a batch flush after timeout
   */
  private scheduleBatchFlush(): void {
    if (this.batchTimeout) {
      return // Already scheduled
    }

    this.batchTimeout = setTimeout(() => {
      void this.flushBatch()
    }, this.batchTimeoutMs)
  }

  /**
   * Get events with filtering and pagination
   */
  public async getEvents(
    options: EventQueryOptions = {}
  ): Promise<RunEventRow[]> {
    let query = this.client.from('run_events').select('*')

    if (options.runId) {
      query = query.eq('run_id', options.runId)
    }

    if (options.eventTypes && options.eventTypes.length > 0) {
      query = query.in('event_type', options.eventTypes)
    }

    if (options.startDate) {
      query = query.gte('timestamp', options.startDate.toISOString())
    }

    if (options.endDate) {
      query = query.lte('timestamp', options.endDate.toISOString())
    }

    if (options.orderBy) {
      query = query.order(options.orderBy, {
        ascending: options.ascending ?? false,
      })
    } else {
      query = query.order('timestamp', { ascending: false })
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
      throw new Error(`Failed to get events: ${error.message}`)
    }

    return data || []
  }

  /**
   * Get events for a specific run
   */
  public async getEventsForRun(
    runId: string,
    options: Omit<EventQueryOptions, 'runId'> = {}
  ): Promise<RunEventRow[]> {
    return this.getEvents({ ...options, runId })
  }

  /**
   * Get the latest events across all runs
   */
  public async getLatestEvents(limit = 100): Promise<RunEventRow[]> {
    return this.getEvents({
      limit,
      orderBy: 'timestamp',
      ascending: false,
    })
  }

  /**
   * Count events with optional filtering
   */
  public async countEvents(
    options: Omit<
      EventQueryOptions,
      'limit' | 'offset' | 'orderBy' | 'ascending'
    > = {}
  ): Promise<number> {
    let query = this.client
      .from('run_events')
      .select('*', { count: 'exact', head: true })

    if (options.runId) {
      query = query.eq('run_id', options.runId)
    }

    if (options.eventTypes && options.eventTypes.length > 0) {
      query = query.in('event_type', options.eventTypes)
    }

    if (options.startDate) {
      query = query.gte('timestamp', options.startDate.toISOString())
    }

    if (options.endDate) {
      query = query.lte('timestamp', options.endDate.toISOString())
    }

    const { count, error } = await query

    if (error) {
      throw new Error(`Failed to count events: ${error.message}`)
    }

    return count || 0
  }

  /**
   * Get event statistics
   */
  public async getEventStats(
    options: Omit<
      EventQueryOptions,
      'limit' | 'offset' | 'orderBy' | 'ascending'
    > = {}
  ): Promise<EventStats> {
    let query = this.client
      .from('run_events')
      .select('event_type, run_id, timestamp')

    if (options.runId) {
      query = query.eq('run_id', options.runId)
    }
    if (options.eventTypes && options.eventTypes.length > 0) {
      query = query.in('event_type', options.eventTypes)
    }
    if (options.startDate) {
      query = query.gte('timestamp', options.startDate.toISOString())
    }
    if (options.endDate) {
      query = query.lte('timestamp', options.endDate.toISOString())
    }

    const { data, error } = await query

    if (error) {
      throw new Error(`Failed to get event statistics: ${error.message}`)
    }

    const stats: EventStats = {
      totalEvents: data?.length || 0,
      eventsByType: {},
      eventsByRunId: {},
      dateRange: {
        earliest: null,
        latest: null,
      },
    }

    if (data && data.length > 0) {
      let earliestTime = new Date()
      let latestTime = new Date(0)

      data.forEach((event: any) => {
        // Count by event type
        stats.eventsByType[event.event_type] =
          (stats.eventsByType[event.event_type] || 0) + 1

        // Count by run ID
        stats.eventsByRunId[event.run_id] =
          (stats.eventsByRunId[event.run_id] || 0) + 1

        // Track date range
        const eventTime = new Date(event.timestamp)
        if (eventTime < earliestTime) {
          earliestTime = eventTime
        }
        if (eventTime > latestTime) {
          latestTime = eventTime
        }
      })

      stats.dateRange.earliest = earliestTime
      stats.dateRange.latest = latestTime
    }

    return stats
  }

  /**
   * Delete events for a specific run
   */
  public async deleteEventsForRun(runId: string): Promise<number> {
    const { count, error } = await this.client
      .from('run_events')
      .delete({ count: 'exact' })
      .eq('run_id', runId)

    if (error) {
      throw new Error(`Failed to delete events for run: ${error.message}`)
    }

    return count || 0
  }

  /**
   * Delete events older than the specified date
   */
  public async deleteOldEvents(beforeDate: Date): Promise<number> {
    const { count, error } = await this.client
      .from('run_events')
      .delete({ count: 'exact' })
      .lt('timestamp', beforeDate.toISOString())

    if (error) {
      throw new Error(`Failed to delete old events: ${error.message}`)
    }

    return count || 0
  }

  /**
   * Get distinct event types
   */
  public async getEventTypes(): Promise<string[]> {
    const { data, error } = await this.client
      .from('run_events')
      .select('event_type')
      .order('event_type')

    if (error) {
      throw new Error(`Failed to get event types: ${error.message}`)
    }

    if (!data) {
      return []
    }

    // Extract unique event types
    const eventTypes = new Set(data.map((row) => row.event_type))
    return Array.from(eventTypes).sort()
  }

  /**
   * Clean up resources and flush any pending events
   */
  public async destroy(): Promise<void> {
    if (this.batchTimeout) {
      clearTimeout(this.batchTimeout)
      this.batchTimeout = null
    }

    // Flush any remaining events
    await this.flushBatch()
  }

  /**
   * Get buffer status
   */
  public getBufferStatus(): {
    totalBuffered: number
    buffersByRunId: Record<string, number>
    hasScheduledFlush: boolean
  } {
    const buffersByRunId: Record<string, number> = {}
    let totalBuffered = 0

    for (const [runId, events] of this.batchBuffer.entries()) {
      buffersByRunId[runId] = events.length
      totalBuffered += events.length
    }

    return {
      totalBuffered,
      buffersByRunId,
      hasScheduledFlush: this.batchTimeout !== null,
    }
  }
}
