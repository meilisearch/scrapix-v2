import express, { Request, Response, NextFunction, Router } from 'express'
import { RunManager, EventStorage, ConfigManager } from '@scrapix/core'
import { z } from 'zod'
import { Log } from 'crawlee'
import rateLimit from 'express-rate-limit'

const router: Router = express.Router()
const log = new Log({ prefix: 'RunsAPI' })

/**
 * Rate limiting for run operations
 */
const runLimiter = rateLimit({
  windowMs: 15 * 60 * 1000, // 15 minutes
  max: 200, // Limit each IP to 200 requests per windowMs (more generous for monitoring)
  message: 'Too many run requests from this IP, please try again later.',
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
const CreateRunSchema = z.object({
  configId: z.string().uuid().optional(),
  configSnapshot: z.any().optional(), // Will be validated by ConfigManager if provided
  metadata: z.record(z.any()).optional(),
})

const ListRunsQuerySchema = z.object({
  configId: z.string().uuid().optional(),
  status: z
    .enum(['pending', 'running', 'completed', 'failed', 'cancelled'])
    .optional(),
  limit: z
    .string()
    .transform((val) => Math.min(parseInt(val) || 20, 100))
    .optional(),
  offset: z
    .string()
    .transform((val) => parseInt(val) || 0)
    .optional(),
  orderBy: z.enum(['started_at', 'completed_at', 'duration_ms']).optional(),
  ascending: z
    .string()
    .transform((val) => val === 'true')
    .optional(),
})

const ListEventsQuerySchema = z.object({
  eventTypes: z
    .string()
    .transform((val) => val.split(',').filter(Boolean))
    .optional(),
  limit: z
    .string()
    .transform((val) => Math.min(parseInt(val) || 100, 1000))
    .optional(),
  offset: z
    .string()
    .transform((val) => parseInt(val) || 0)
    .optional(),
  startDate: z
    .string()
    .transform((val) => new Date(val))
    .optional(),
  endDate: z
    .string()
    .transform((val) => new Date(val))
    .optional(),
  orderBy: z.enum(['timestamp', 'event_type']).optional(),
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
  log.error('Runs API error', {
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
router.use(runLimiter)
router.use(requireSupabase)

/**
 * POST /runs
 * Create a new run
 */
router.post(
  '/',
  validateBody(CreateRunSchema),
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const { configId, configSnapshot, metadata } = req.body

      // TODO: Extract user ID from authentication when implemented
      const createdBy = (req.headers['x-user-id'] as string) || undefined

      // If configId is provided, validate it exists and get the config
      let finalConfigSnapshot = configSnapshot
      if (configId && !configSnapshot) {
        const configManager = new ConfigManager()
        const config = await configManager.get(configId)
        if (!config) {
          return res.status(404).json({
            status: 'error',
            error: {
              message: 'Configuration not found',
            },
          })
        }
        finalConfigSnapshot = config.config
      }

      const runManager = new RunManager()
      const runId = await runManager.createRun({
        configId,
        configSnapshot: finalConfigSnapshot,
        createdBy,
        metadata,
      })

      log.info('Run created', { runId, configId, createdBy })

      res.status(201).json({
        status: 'success',
        data: {
          id: runId,
          configId,
          createdBy,
          metadata,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /runs
 * List runs with filtering and pagination
 */
router.get(
  '/',
  validateQuery(ListRunsQuerySchema),
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const runManager = new RunManager()
      const { configId, status, limit, offset, orderBy, ascending } =
        req.query as any

      // TODO: Extract user ID from authentication when implemented
      const createdBy = (req.headers['x-user-filter'] as string) || undefined

      const runs = await runManager.getRuns({
        configId,
        status,
        createdBy,
        limit,
        offset,
        orderBy,
        ascending,
      })

      res.json({
        status: 'success',
        data: {
          runs: runs.map((run) => ({
            id: run.id,
            configId: run.config_id,
            status: run.status,
            startedAt: run.started_at,
            completedAt: run.completed_at,
            durationMs: run.duration_ms,
            totalPagesCrawled: run.total_pages_crawled,
            totalPagesIndexed: run.total_pages_indexed,
            totalDocumentsSent: run.total_documents_sent,
            errorMessage: run.error_message,
            createdBy: run.created_by,
            metadata: run.metadata,
          })),
          pagination: {
            limit: limit || 20,
            offset: offset || 0,
            hasMore: runs.length === (limit || 20),
          },
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /runs/:id
 * Get a specific run
 */
router.get('/:id', async (req: Request, res: Response, next: NextFunction) => {
  try {
    const runManager = new RunManager()
    const { id } = req.params

    const run = await runManager.getRun(id)
    if (!run) {
      return res.status(404).json({
        status: 'error',
        error: {
          message: 'Run not found',
        },
      })
    }

    res.json({
      status: 'success',
      data: {
        id: run.id,
        configId: run.config_id,
        configSnapshot: run.config_snapshot,
        status: run.status,
        startedAt: run.started_at,
        completedAt: run.completed_at,
        durationMs: run.duration_ms,
        totalPagesCrawled: run.total_pages_crawled,
        totalPagesIndexed: run.total_pages_indexed,
        totalDocumentsSent: run.total_documents_sent,
        errorMessage: run.error_message,
        createdBy: run.created_by,
        metadata: run.metadata,
      },
    })
  } catch (error) {
    next(error)
  }
})

/**
 * POST /runs/:id/start
 * Start a run
 */
router.post(
  '/:id/start',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const runManager = new RunManager(req.params.id)

      // Check if run exists
      const run = await runManager.getRun()
      if (!run) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Run not found',
          },
        })
      }

      if (run.status !== 'pending') {
        return res.status(400).json({
          status: 'error',
          error: {
            message: `Cannot start run with status '${run.status}'. Run must be in 'pending' status.`,
          },
        })
      }

      await runManager.startRun()

      log.info('Run started', { runId: req.params.id })

      res.json({
        status: 'success',
        message: 'Run started successfully',
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * POST /runs/:id/cancel
 * Cancel a run
 */
router.post(
  '/:id/cancel',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const runManager = new RunManager(req.params.id)
      const { reason } = req.body

      // Check if run exists
      const run = await runManager.getRun()
      if (!run) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Run not found',
          },
        })
      }

      if (
        run.status === 'completed' ||
        run.status === 'failed' ||
        run.status === 'cancelled'
      ) {
        return res.status(400).json({
          status: 'error',
          error: {
            message: `Cannot cancel run with status '${run.status}'. Run is already finished.`,
          },
        })
      }

      await runManager.cancelRun(reason)

      log.info('Run cancelled', { runId: req.params.id, reason })

      res.json({
        status: 'success',
        message: 'Run cancelled successfully',
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /runs/:id/events
 * Get events for a specific run
 */
router.get(
  '/:id/events',
  validateQuery(ListEventsQuerySchema),
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const eventStorage = new EventStorage()
      const { id } = req.params
      const {
        eventTypes,
        limit,
        offset,
        startDate,
        endDate,
        orderBy,
        ascending,
      } = req.query as any

      // Check if run exists
      const runManager = new RunManager(id)
      const run = await runManager.getRun()
      if (!run) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Run not found',
          },
        })
      }

      const events = await eventStorage.getEventsForRun(id, {
        eventTypes,
        limit,
        offset,
        startDate,
        endDate,
        orderBy,
        ascending,
      })

      const totalEvents = await eventStorage.countEvents({
        runId: id,
        eventTypes,
        startDate,
        endDate,
      })

      res.json({
        status: 'success',
        data: {
          runId: id,
          events: events.map((event) => ({
            id: event.id,
            eventType: event.event_type,
            eventData: event.event_data,
            timestamp: event.timestamp,
            metadata: event.metadata,
          })),
          pagination: {
            total: totalEvents,
            limit: limit || 100,
            offset: offset || 0,
            hasMore: events.length === (limit || 100),
          },
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /runs/:id/stats
 * Get statistics for a specific run
 */
router.get(
  '/:id/stats',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const eventStorage = new EventStorage()
      const { id } = req.params

      // Check if run exists
      const runManager = new RunManager(id)
      const run = await runManager.getRun()
      if (!run) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Run not found',
          },
        })
      }

      const eventStats = await eventStorage.getEventStats({ runId: id })

      res.json({
        status: 'success',
        data: {
          runId: id,
          runStats: {
            status: run.status,
            startedAt: run.started_at,
            completedAt: run.completed_at,
            durationMs: run.duration_ms,
            totalPagesCrawled: run.total_pages_crawled,
            totalPagesIndexed: run.total_pages_indexed,
            totalDocumentsSent: run.total_documents_sent,
            errorMessage: run.error_message,
          },
          eventStats,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * DELETE /runs/:id/events
 * Delete events for a specific run
 */
router.delete(
  '/:id/events',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const eventStorage = new EventStorage()
      const { id } = req.params

      // Check if run exists
      const runManager = new RunManager(id)
      const run = await runManager.getRun()
      if (!run) {
        return res.status(404).json({
          status: 'error',
          error: {
            message: 'Run not found',
          },
        })
      }

      const deletedCount = await eventStorage.deleteEventsForRun(id)

      log.info('Run events deleted', { runId: id, deletedCount })

      res.json({
        status: 'success',
        data: {
          runId: id,
          deletedCount,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /runs/stats
 * Get overall run statistics
 */
router.get(
  '/stats',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const eventStorage = new EventStorage()

      const configId = req.query.configId as string | undefined

      const eventStats = await eventStorage.getEventStats(
        configId ? { runId: configId } : {}
      )

      res.json({
        status: 'success',
        data: {
          eventStats,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * GET /runs/events/types
 * Get all event types across runs
 */
router.get(
  '/events/types',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const eventStorage = new EventStorage()
      const eventTypes = await eventStorage.getEventTypes()

      res.json({
        status: 'success',
        data: {
          eventTypes,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

/**
 * DELETE /runs/events/old
 * Delete old events (cleanup utility)
 */
router.delete(
  '/events/old',
  async (req: Request, res: Response, next: NextFunction) => {
    try {
      const eventStorage = new EventStorage()
      const { beforeDays = 30 } = req.query

      const beforeDate = new Date()
      beforeDate.setDate(beforeDate.getDate() - parseInt(beforeDays as string))

      const deletedCount = await eventStorage.deleteOldEvents(beforeDate)

      log.info('Old events deleted', { beforeDate, deletedCount })

      res.json({
        status: 'success',
        data: {
          beforeDate,
          deletedCount,
        },
      })
    } catch (error) {
      next(error)
    }
  }
)

// Error handler (must be last)
router.use(handleError)

export default router
