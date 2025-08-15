import express, { Request, Response, NextFunction, Router } from 'express'
import { ConfigManager } from '@scrapix/core'
import { z } from 'zod'
import { Log } from 'crawlee'
import rateLimit from 'express-rate-limit'

const router: Router = express.Router()
const log = new Log({ prefix: 'ConfigsAPI' })

/**
 * Rate limiting for config operations
 */
const configLimiter = rateLimit({
  windowMs: 15 * 60 * 1000, // 15 minutes
  max: 100, // Limit each IP to 100 requests per windowMs
  message: 'Too many config requests from this IP, please try again later.',
  standardHeaders: true,
  legacyHeaders: false,
  validate: false,
  skip: (req) => {
    // Skip rate limiting if we can't determine the IP (fail open)
    if (!req.ip && !req.ips?.length) {
      log.warning('Unable to determine client IP for rate limiting')
      return true
    }
    return false
  },
})

/**
 * Request validation schemas
 */
const CreateConfigSchema = z.object({
  name: z.string().min(1).max(255),
  description: z.string().max(1000).optional(),
  config: z.any(), // Will be validated by ConfigManager using ConfigSchema
  tags: z.array(z.string()).optional(),
})

const UpdateConfigSchema = z.object({
  name: z.string().min(1).max(255).optional(),
  description: z.string().max(1000).optional(),
  config: z.any().optional(), // Will be validated by ConfigManager using ConfigSchema
  isActive: z.boolean().optional(),
  tags: z.array(z.string()).optional(),
})

const ListConfigsQuerySchema = z.object({
  isActive: z
    .string()
    .transform((val) => val === 'true')
    .optional(),
  tags: z
    .string()
    .transform((val) => val.split(',').filter(Boolean))
    .optional(),
  search: z.string().optional(),
  limit: z
    .string()
    .transform((val) => parseInt(val))
    .optional(),
  offset: z
    .string()
    .transform((val) => parseInt(val))
    .optional(),
  orderBy: z.enum(['created_at', 'updated_at', 'name']).optional(),
  ascending: z
    .string()
    .transform((val) => val === 'true')
    .optional(),
})

/**
 * Middleware for request validation
 */
const validateBody = (schema: z.ZodSchema) => {
  return (req: Request, res: Response, next: NextFunction) => {
    try {
      req.body = schema.parse(req.body)
      next()
    } catch (error) {
      if (error instanceof z.ZodError) {
        res.status(400).json({
          status: 'error',
          error: {
            message: 'Invalid request body',
            details: error.errors,
          },
        })
      } else {
        next(error)
      }
    }
  }
}

const validateQuery = (schema: z.ZodSchema) => {
  return (req: Request, res: Response, next: NextFunction) => {
    try {
      req.query = schema.parse(req.query)
      next()
    } catch (error) {
      if (error instanceof z.ZodError) {
        res.status(400).json({
          status: 'error',
          error: {
            message: 'Invalid query parameters',
            details: error.errors,
          },
        })
      } else {
        next(error)
      }
    }
  }
}

/**
 * Error handler middleware
 */
const handleError = (
  error: Error,
  req: Request,
  res: Response,
  _next: NextFunction
) => {
  log.error('Config API error', {
    error: error.message,
    path: req.path,
    method: req.method,
  })

  if (error.message.includes('not found')) {
    res.status(404).json({
      status: 'error',
      error: {
        message: error.message,
      },
    })
  } else if (error.message.includes('already exists')) {
    res.status(409).json({
      status: 'error',
      error: {
        message: error.message,
      },
    })
  } else if (error.message.includes('Invalid configuration')) {
    res.status(400).json({
      status: 'error',
      error: {
        message: error.message,
      },
    })
  } else {
    res.status(500).json({
      status: 'error',
      error: {
        message: 'Internal server error',
        ...(process.env.NODE_ENV !== 'production' && {
          details: error.message,
        }),
      },
    })
  }
}

/**
 * Check if Supabase is configured middleware
 */
const requireSupabase = (req: Request, res: Response, next: NextFunction) => {
  if (!process.env.SUPABASE_URL || !process.env.SUPABASE_ANON_KEY) {
    return res.status(501).json({
      status: 'error',
      error: {
        message:
          'Supabase integration not configured. Please set SUPABASE_URL and SUPABASE_ANON_KEY environment variables.',
      },
    })
  }
  next()
}

// Apply middleware
router.use(configLimiter)
router.use(requireSupabase)

/**
 * POST /configs
 * Create a new configuration
 */
router.post(
  '/',
  validateBody(CreateConfigSchema),
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const { name, description, config, tags } = req.body

      // TODO: Extract user ID from authentication when implemented
      const createdBy = (req.headers['x-user-id'] as string) || undefined

      const configId = await configManager.create({
        name,
        description,
        config,
        tags,
        createdBy,
      })

      log.info('Configuration created', { configId, name, createdBy })

      res.status(201).json({
        status: 'success',
        data: {
          id: configId,
          name,
          description,
          tags,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /configs
 * List configurations with filtering and pagination
 */
router.get(
  '/',
  validateQuery(ListConfigsQuerySchema),
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const { isActive, tags, search, limit, offset, orderBy, ascending } =
        req.query as any

      // TODO: Extract user ID from authentication when implemented
      const createdBy = (req.headers['x-user-filter'] as string) || undefined

      const configs = await configManager.list({
        isActive,
        tags,
        search,
        limit,
        offset,
        orderBy,
        ascending,
        createdBy,
      })

      const total = await configManager.count({
        isActive,
        tags,
        search,
        createdBy,
      })

      res.json({
        status: 'success',
        data: {
          configs: configs.map((config) => ({
            id: config.id,
            name: config.name,
            description: config.description,
            isActive: config.is_active,
            tags: config.tags,
            createdAt: config.created_at,
            updatedAt: config.updated_at,
            createdBy: config.created_by,
          })),
          pagination: {
            total,
            limit: limit || null,
            offset: offset || 0,
            hasMore: limit ? (offset || 0) + limit < total : false,
          },
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /configs/:id
 * Get a specific configuration
 */
router.get('/:id', async (req: Request, res: Response, next: NextFunction) => {
  try {
    const configManager = new ConfigManager()
    const { id } = req.params

    const config = await configManager.get(id)
    if (!config) {
      return res.status(404).json({
        status: 'error',
        error: {
          message: 'Configuration not found',
        },
      })
    }

    res.json({
      status: 'success',
      data: {
        id: config.id,
        name: config.name,
        description: config.description,
        config: config.config,
        isActive: config.is_active,
        tags: config.tags,
        createdAt: config.created_at,
        updatedAt: config.updated_at,
        createdBy: config.created_by,
      },
    })
  } catch (error) {
    next(error)
  }
})

/**
 * PUT /configs/:id
 * Update a configuration
 */
router.put(
  '/:id',
  validateBody(UpdateConfigSchema),
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const { id } = req.params
      const updates = req.body

      // Check if config exists
      const existingConfig = await configManager.get(id)
      if (!existingConfig) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Configuration not found',
          },
        })
      }

      await configManager.update(id, updates)

      log.info('Configuration updated', { configId: id, updates })

      // Return updated config
      const updatedConfig = await configManager.get(id)
      res.json({
        status: 'success',
        data: {
          id: updatedConfig!.id,
          name: updatedConfig!.name,
          description: updatedConfig!.description,
          config: updatedConfig!.config,
          isActive: updatedConfig!.is_active,
          tags: updatedConfig!.tags,
          createdAt: updatedConfig!.created_at,
          updatedAt: updatedConfig!.updated_at,
          createdBy: updatedConfig!.created_by,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * DELETE /configs/:id
 * Delete a configuration (hard delete)
 */
router.delete(
  '/:id',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const { id } = req.params

      // Check if config exists
      const existingConfig = await configManager.get(id)
      if (!existingConfig) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Configuration not found',
          },
        })
      }

      await configManager.delete(id)

      log.info('Configuration deleted', { configId: id })

      res.json({
        status: 'success',
        message: 'Configuration deleted successfully',
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * POST /configs/:id/duplicate
 * Duplicate a configuration
 */
router.post(
  '/:id/duplicate',
  validateBody(
    z.object({
      name: z.string().min(1).max(255),
      description: z.string().max(1000).optional(),
      tags: z.array(z.string()).optional(),
    })
  ),
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const { id } = req.params
      const { name, description, tags } = req.body

      // TODO: Extract user ID from authentication when implemented
      const createdBy = (req.headers['x-user-id'] as string) || undefined

      const newConfigId = await configManager.duplicate(id, name, {
        description,
        tags,
        createdBy,
      })

      log.info('Configuration duplicated', {
        originalId: id,
        newId: newConfigId,
        newName: name,
      })

      res.status(201).json({
        status: 'success',
        data: {
          id: newConfigId,
          name,
          description,
          tags,
          originalId: id,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * POST /configs/:id/restore
 * Restore a soft-deleted configuration
 */
router.post(
  '/:id/restore',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const { id } = req.params

      // Check if config exists
      const existingConfig = await configManager.get(id)
      if (!existingConfig) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Configuration not found',
          },
        })
      }

      await configManager.restore(id)

      log.info('Configuration restored', { configId: id })

      res.json({
        status: 'success',
        message: 'Configuration restored successfully',
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * POST /configs/validate
 * Validate a configuration without saving it
 */
router.post(
  '/validate',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const { config } = req.body

      if (!config) {
        return res.status(400).json({
          status: 'error',
          error: {
            message: 'Config is required for validation',
          },
        })
      }

      const validation = configManager.validateConfig(config)

      res.json({
        status: 'success',
        data: {
          valid: validation.valid,
          ...(validation.errors && { errors: validation.errors }),
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /configs/stats
 * Get configuration statistics
 */
router.get(
  '/stats',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const configManager = new ConfigManager()
      const stats = await configManager.getStats()

      res.json({
        status: 'success',
        data: stats,
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /configs/tags
 * Get all unique tags
 */
router.get('/tags', async (req: Request, res: Response, next: NextFunction) => {
  try {
    const configManager = new ConfigManager()
    const tags = await configManager.getAllTags()

    res.json({
      status: 'success',
      data: {
        tags,
      },
    })
  } catch (error) {
    next(error)
  }
})

// Error handler (must be last)
router.use(handleError)

export default router
