import { v4 as uuidv4 } from 'uuid'
import SupabaseClientManager, {
  ConfigRow,
  ConfigInsert,
  ConfigUpdate,
} from './client'
import { Config, ConfigSchema } from '../types'
import { z } from 'zod'

/**
 * Interface for config creation options
 */
export interface CreateConfigOptions {
  name: string
  description?: string
  config: Config
  createdBy?: string
  isActive?: boolean
  tags?: string[]
}

/**
 * Interface for config update options
 */
export interface UpdateConfigOptions {
  name?: string
  description?: string
  config?: Config
  isActive?: boolean
  tags?: string[]
}

/**
 * Interface for config listing options
 */
export interface ListConfigsOptions {
  isActive?: boolean
  createdBy?: string
  tags?: string[]
  limit?: number
  offset?: number
  search?: string
  orderBy?: 'created_at' | 'updated_at' | 'name'
  ascending?: boolean
}

/**
 * ConfigManager class handles CRUD operations for crawler configurations in Supabase
 * Provides validation, error handling, and convenient methods for config management
 */
export class ConfigManager {
  private client = SupabaseClientManager.getInstance()

  /**
   * Create a new configuration
   */
  public async create(options: CreateConfigOptions): Promise<string> {
    // Validate the configuration
    try {
      ConfigSchema.parse(options.config)
    } catch (error) {
      if (error instanceof z.ZodError) {
        throw new Error(
          `Invalid configuration: ${error.errors.map((e) => `${e.path.join('.')}: ${e.message}`).join(', ')}`
        )
      }
      throw error
    }

    const configId = uuidv4()

    const configData: ConfigInsert = {
      id: configId,
      name: options.name,
      description: options.description,
      config: options.config,
      created_by: options.createdBy,
      is_active: options.isActive ?? true,
      tags: options.tags,
    }

    const { error } = await this.client.from('configs').insert(configData)

    if (error) {
      if (error.code === '23505') {
        // Unique violation
        throw new Error(
          `A configuration with the name "${options.name}" already exists`
        )
      }
      throw new Error(`Failed to create configuration: ${error.message}`)
    }

    return configId
  }

  /**
   * Get a configuration by ID
   */
  public async get(id: string): Promise<ConfigRow | null> {
    const { data, error } = await this.client
      .from('configs')
      .select('*')
      .eq('id', id)
      .single()

    if (error) {
      if (error.code === 'PGRST116') {
        // No rows found
        return null
      }
      throw new Error(`Failed to get configuration: ${error.message}`)
    }

    return data
  }

  /**
   * Get a configuration by name
   */
  public async getByName(name: string): Promise<ConfigRow | null> {
    const { data, error } = await this.client
      .from('configs')
      .select('*')
      .eq('name', name)
      .eq('is_active', true)
      .single()

    if (error) {
      if (error.code === 'PGRST116') {
        // No rows found
        return null
      }
      throw new Error(`Failed to get configuration by name: ${error.message}`)
    }

    return data
  }

  /**
   * Update a configuration
   */
  public async update(id: string, options: UpdateConfigOptions): Promise<void> {
    // Validate the configuration if provided
    if (options.config) {
      try {
        ConfigSchema.parse(options.config)
      } catch (error) {
        if (error instanceof z.ZodError) {
          throw new Error(
            `Invalid configuration: ${error.errors.map((e) => `${e.path.join('.')}: ${e.message}`).join(', ')}`
          )
        }
        throw error
      }
    }

    const updateData: ConfigUpdate = {
      ...options,
      updated_at: new Date().toISOString(),
    }

    const { error } = await this.client
      .from('configs')
      .update(updateData)
      .eq('id', id)

    if (error) {
      if (error.code === '23505') {
        // Unique violation
        throw new Error(
          `A configuration with the name "${options.name}" already exists`
        )
      }
      throw new Error(`Failed to update configuration: ${error.message}`)
    }
  }

  /**
   * Delete a configuration (hard delete)
   */
  public async delete(id: string): Promise<void> {
    const { error } = await this.client.from('configs').delete().eq('id', id)

    if (error) {
      throw new Error(`Failed to delete configuration: ${error.message}`)
    }
  }

  /**
   * Soft delete a configuration (mark as inactive)
   */
  public async softDelete(id: string): Promise<void> {
    await this.update(id, { isActive: false })
  }

  /**
   * Restore a soft-deleted configuration
   */
  public async restore(id: string): Promise<void> {
    await this.update(id, { isActive: true })
  }

  /**
   * List configurations with optional filtering
   */
  public async list(options: ListConfigsOptions = {}): Promise<ConfigRow[]> {
    let query = this.client.from('configs').select('*')

    if (options.isActive !== undefined) {
      query = query.eq('is_active', options.isActive)
    }

    if (options.createdBy) {
      query = query.eq('created_by', options.createdBy)
    }

    if (options.tags && options.tags.length > 0) {
      query = query.contains('tags', options.tags)
    }

    if (options.search) {
      query = query.or(
        `name.ilike.%${options.search}%,description.ilike.%${options.search}%`
      )
    }

    if (options.orderBy) {
      query = query.order(options.orderBy, {
        ascending: options.ascending ?? false,
      })
    } else {
      query = query.order('created_at', { ascending: false })
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
      throw new Error(`Failed to list configurations: ${error.message}`)
    }

    return data || []
  }

  /**
   * Count configurations with optional filtering
   */
  public async count(
    options: Pick<
      ListConfigsOptions,
      'isActive' | 'createdBy' | 'tags' | 'search'
    > = {}
  ): Promise<number> {
    let query = this.client
      .from('configs')
      .select('*', { count: 'exact', head: true })

    if (options.isActive !== undefined) {
      query = query.eq('is_active', options.isActive)
    }

    if (options.createdBy) {
      query = query.eq('created_by', options.createdBy)
    }

    if (options.tags && options.tags.length > 0) {
      query = query.contains('tags', options.tags)
    }

    if (options.search) {
      query = query.or(
        `name.ilike.%${options.search}%,description.ilike.%${options.search}%`
      )
    }

    const { count, error } = await query

    if (error) {
      throw new Error(`Failed to count configurations: ${error.message}`)
    }

    return count || 0
  }

  /**
   * Check if a configuration name is available
   */
  public async isNameAvailable(
    name: string,
    excludeId?: string
  ): Promise<boolean> {
    let query = this.client.from('configs').select('id').eq('name', name)

    if (excludeId) {
      query = query.neq('id', excludeId)
    }

    const { data, error } = await query

    if (error) {
      throw new Error(`Failed to check name availability: ${error.message}`)
    }

    return !data || data.length === 0
  }

  /**
   * Get configurations by tags
   */
  public async getByTags(tags: string[]): Promise<ConfigRow[]> {
    return this.list({ tags, isActive: true })
  }

  /**
   * Get all unique tags
   */
  public async getAllTags(): Promise<string[]> {
    const { data, error } = await this.client
      .from('configs')
      .select('tags')
      .eq('is_active', true)
      .not('tags', 'is', null)

    if (error) {
      throw new Error(`Failed to get tags: ${error.message}`)
    }

    if (!data) {
      return []
    }

    // Flatten and deduplicate tags
    const allTags = new Set<string>()
    data.forEach((row) => {
      if (row.tags) {
        row.tags.forEach((tag: string) => allTags.add(tag))
      }
    })

    return Array.from(allTags).sort()
  }

  /**
   * Duplicate a configuration
   */
  public async duplicate(
    id: string,
    newName: string,
    options?: {
      description?: string
      createdBy?: string
      tags?: string[]
    }
  ): Promise<string> {
    const originalConfig = await this.get(id)
    if (!originalConfig) {
      throw new Error('Configuration not found')
    }

    return this.create({
      name: newName,
      description: options?.description || `Copy of ${originalConfig.name}`,
      config: originalConfig.config,
      createdBy: options?.createdBy,
      tags: options?.tags || originalConfig.tags || undefined,
    })
  }

  /**
   * Validate a configuration without saving it
   */
  public validateConfig(config: Config): { valid: boolean; errors?: string[] } {
    try {
      ConfigSchema.parse(config)
      return { valid: true }
    } catch (error) {
      if (error instanceof z.ZodError) {
        return {
          valid: false,
          errors: error.errors.map((e) => `${e.path.join('.')}: ${e.message}`),
        }
      }
      return {
        valid: false,
        errors: ['Unknown validation error'],
      }
    }
  }

  /**
   * Get configuration statistics
   */
  public async getStats(): Promise<{
    total: number
    active: number
    inactive: number
    totalByCreatedBy: Record<string, number>
    totalByTags: Record<string, number>
  }> {
    const { data, error } = await this.client
      .from('configs')
      .select('is_active, created_by, tags')

    if (error) {
      throw new Error(
        `Failed to get configuration statistics: ${error.message}`
      )
    }

    const stats = {
      total: data?.length || 0,
      active: 0,
      inactive: 0,
      totalByCreatedBy: {} as Record<string, number>,
      totalByTags: {} as Record<string, number>,
    }

    if (data) {
      data.forEach((row) => {
        if (row.is_active) {
          stats.active++
        } else {
          stats.inactive++
        }

        if (row.created_by) {
          stats.totalByCreatedBy[row.created_by] =
            (stats.totalByCreatedBy[row.created_by] || 0) + 1
        }

        if (row.tags) {
          row.tags.forEach((tag: string) => {
            stats.totalByTags[tag] = (stats.totalByTags[tag] || 0) + 1
          })
        }
      })
    }

    return stats
  }
}
