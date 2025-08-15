import { createClient, SupabaseClient } from '@supabase/supabase-js'
import { Config } from '../types'

/**
 * Database table type definitions matching Supabase schema
 */
export interface Database {
  public: {
    Tables: {
      configs: {
        Row: ConfigRow
        Insert: ConfigInsert
        Update: ConfigUpdate
      }
      runs: {
        Row: RunRow
        Insert: RunInsert
        Update: RunUpdate
      }
      run_events: {
        Row: RunEventRow
        Insert: RunEventInsert
        Update: RunEventUpdate
      }
      crons: {
        Row: CronRow
        Insert: CronInsert
        Update: CronUpdate
      }
      run_logs: {
        Row: RunLogRow
        Insert: RunLogInsert
        Update: RunLogUpdate
      }
    }
  }
}

/**
 * Config table types
 */
export interface ConfigRow {
  id: string
  name: string
  description: string | null
  config: Config
  created_at: string
  updated_at: string
  created_by: string | null
  is_active: boolean
  tags: string[] | null
}

export interface ConfigInsert {
  id?: string
  name: string
  description?: string | null
  config: Config
  created_by?: string | null
  is_active?: boolean
  tags?: string[] | null
}

export interface ConfigUpdate {
  id?: string
  name?: string
  description?: string | null
  config?: Config
  updated_at?: string
  created_by?: string | null
  is_active?: boolean
  tags?: string[] | null
}

/**
 * Run table types
 */
export interface RunRow {
  id: string
  config_id: string | null
  config_snapshot: Config | null
  status: RunStatus
  started_at: string
  completed_at: string | null
  error_message: string | null
  total_pages_crawled: number
  total_pages_indexed: number
  total_documents_sent: number
  duration_ms: number | null
  created_by: string | null
  metadata: Record<string, any> | null
}

export interface RunInsert {
  id?: string
  config_id?: string | null
  config_snapshot?: Config | null
  status?: RunStatus
  started_at?: string
  completed_at?: string | null
  error_message?: string | null
  total_pages_crawled?: number
  total_pages_indexed?: number
  total_documents_sent?: number
  duration_ms?: number | null
  created_by?: string | null
  metadata?: Record<string, any> | null
}

export interface RunUpdate {
  config_id?: string | null
  config_snapshot?: Config | null
  status?: RunStatus
  started_at?: string | null
  completed_at?: string | null
  error_message?: string | null
  total_pages_crawled?: number
  total_pages_indexed?: number
  total_documents_sent?: number
  duration_ms?: number | null
  metadata?: Record<string, any> | null
}

export type RunStatus =
  | 'pending'
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'

/**
 * Run events table types
 */
export interface RunEventRow {
  id: string
  run_id: string
  event_type: string
  event_data: Record<string, any>
  timestamp: string
  metadata: Record<string, any> | null
}

export interface RunEventInsert {
  id?: string
  run_id: string
  event_type: string
  event_data: Record<string, any>
  timestamp?: string
  metadata?: Record<string, any> | null
}

export interface RunEventUpdate {
  event_data?: Record<string, any>
  metadata?: Record<string, any> | null
}

/**
 * Cron table types
 */
export interface CronRow {
  id: string
  name: string
  description: string | null
  config_id: string
  cron_expression: string
  timezone: string
  is_active: boolean
  last_run_at: string | null
  next_run_at: string | null
  created_at: string
  updated_at: string
  created_by: string | null
  failure_count: number
  max_failures: number
  metadata: Record<string, any> | null
}

export interface CronInsert {
  id?: string
  name: string
  description?: string | null
  config_id: string
  cron_expression: string
  timezone?: string
  is_active?: boolean
  last_run_at?: string | null
  next_run_at?: string | null
  created_by?: string | null
  failure_count?: number
  max_failures?: number
  metadata?: Record<string, any> | null
}

export interface CronUpdate {
  name?: string
  description?: string | null
  config_id?: string
  cron_expression?: string
  timezone?: string
  is_active?: boolean
  last_run_at?: string | null
  next_run_at?: string | null
  updated_at?: string
  failure_count?: number
  max_failures?: number
  metadata?: Record<string, any> | null
}

/**
 * Run logs table types
 */
export interface RunLogRow {
  id: string
  run_id: string
  level: LogLevel
  message: string
  timestamp: string
  metadata: Record<string, any> | null
}

export interface RunLogInsert {
  id?: string
  run_id: string
  level: LogLevel
  message: string
  timestamp?: string
  metadata?: Record<string, any> | null
}

export interface RunLogUpdate {
  level?: LogLevel
  message?: string
  metadata?: Record<string, any> | null
}

export type LogLevel = 'debug' | 'info' | 'warning' | 'error'

/**
 * Supabase client singleton
 */
class SupabaseClientManager {
  private static instance: SupabaseClient<Database> | null = null
  private static isInitialized = false

  /**
   * Get the Supabase client instance
   */
  public static getInstance(): SupabaseClient<Database> {
    if (!this.instance) {
      this.initialize()
    }
    return this.instance!
  }

  /**
   * Initialize the Supabase client
   */
  public static initialize(): void {
    if (this.isInitialized) {
      return
    }

    const supabaseUrl = process.env.SUPABASE_URL
    const supabaseAnonKey = process.env.SUPABASE_ANON_KEY

    if (!supabaseUrl || !supabaseAnonKey) {
      throw new Error(
        'Supabase configuration missing. Please set SUPABASE_URL and SUPABASE_ANON_KEY environment variables.'
      )
    }

    this.instance = createClient<Database>(supabaseUrl, supabaseAnonKey, {
      auth: {
        persistSession: false, // Server-side, no session persistence needed
        autoRefreshToken: false,
        detectSessionInUrl: false,
      },
      db: {
        schema: 'public',
      },
    })

    this.isInitialized = true
  }

  /**
   * Reset the client (useful for testing)
   */
  public static reset(): void {
    this.instance = null
    this.isInitialized = false
  }

  /**
   * Check if the client is properly configured
   */
  public static isConfigured(): boolean {
    return !!(process.env.SUPABASE_URL && process.env.SUPABASE_ANON_KEY)
  }
}

/**
 * Helper functions for common database operations
 */
export class SupabaseHelpers {
  private static get client(): SupabaseClient<Database> {
    return SupabaseClientManager.getInstance()
  }

  /**
   * Test database connection
   */
  public static async testConnection(): Promise<boolean> {
    try {
      const { error } = await this.client.from('configs').select('id').limit(1)
      return !error
    } catch {
      return false
    }
  }

  /**
   * Get table row count
   */
  public static async getTableCount(
    tableName: keyof Database['public']['Tables']
  ): Promise<number> {
    const { count, error } = await this.client
      .from(tableName)
      .select('*', { count: 'exact', head: true })

    if (error) {
      throw new Error(`Failed to get ${tableName} count: ${error.message}`)
    }

    return count || 0
  }

  /**
   * Check if a record exists by ID
   */
  public static async recordExists(
    tableName: keyof Database['public']['Tables'],
    id: string
  ): Promise<boolean> {
    const { data, error } = await this.client
      .from(tableName)
      .select('id')
      .eq('id', id)
      .single()

    return !error && !!data
  }

  /**
   * Soft delete (mark as inactive) a config
   */
  public static async softDeleteConfig(id: string): Promise<void> {
    const { error } = await this.client
      .from('configs')
      .update({ is_active: false, updated_at: new Date().toISOString() })
      .eq('id', id)

    if (error) {
      throw new Error(`Failed to soft delete config: ${error.message}`)
    }
  }

  /**
   * Get active configs
   */
  public static async getActiveConfigs(): Promise<ConfigRow[]> {
    const { data, error } = await this.client
      .from('configs')
      .select('*')
      .eq('is_active', true)
      .order('created_at', { ascending: false })

    if (error) {
      throw new Error(`Failed to get active configs: ${error.message}`)
    }

    return data || []
  }

  /**
   * Get recent runs for a config
   */
  public static async getRecentRuns(
    configId: string,
    limit = 10
  ): Promise<RunRow[]> {
    const { data, error } = await this.client
      .from('runs')
      .select('*')
      .eq('config_id', configId)
      .order('started_at', { ascending: false })
      .limit(limit)

    if (error) {
      throw new Error(`Failed to get recent runs: ${error.message}`)
    }

    return data || []
  }

  /**
   * Get run statistics
   */
  public static async getRunStats(configId?: string): Promise<{
    total: number
    completed: number
    failed: number
    running: number
    avgDuration: number | null
  }> {
    let query = this.client.from('runs').select('status, duration_ms')

    if (configId) {
      query = query.eq('config_id', configId)
    }

    const { data, error } = await query

    if (error) {
      throw new Error(`Failed to get run statistics: ${error.message}`)
    }

    if (!data || data.length === 0) {
      return {
        total: 0,
        completed: 0,
        failed: 0,
        running: 0,
        avgDuration: null,
      }
    }

    const total = data.length
    const completed = data.filter((r) => r.status === 'completed').length
    const failed = data.filter((r) => r.status === 'failed').length
    const running = data.filter((r) => r.status === 'running').length

    const completedRuns = data.filter(
      (r) => r.status === 'completed' && r.duration_ms
    )
    const avgDuration =
      completedRuns.length > 0
        ? completedRuns.reduce((sum, r) => sum + (r.duration_ms || 0), 0) /
          completedRuns.length
        : null

    return {
      total,
      completed,
      failed,
      running,
      avgDuration,
    }
  }
}

// Export the client for direct access
export const supabase = SupabaseClientManager.getInstance
export default SupabaseClientManager
