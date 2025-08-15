import * as dotenv from 'dotenv'
dotenv.config()

import express, { Request, Response, NextFunction } from 'express'
import rateLimit from 'express-rate-limit'
import { TaskQueue } from './taskQueue'
import {
  Sender,
  Crawler,
  ConfigSchema,
  ScrapixError,
  ErrorCode,
  logError,
  getConfig,
  Container,
  closeConnectionPools,
  businessMetrics,
  extractCustomerId,
  extractCustomerAttributes,
} from '@scrapix/core'
import { Log } from 'crawlee'
import { z } from 'zod'
import {
  initializeTelemetry,
  tracingMiddleware,
  metricsMiddleware,
} from './telemetry'
import { trace } from '@opentelemetry/api'
import configsRouter from './routes/configs'
import runsRouter from './routes/runs'

const port = getConfig('SERVER', 'DEFAULT_PORT')

const log = new Log({ prefix: 'CrawlerServer' })

// Validation schemas
// const JobIdSchema = z.object({
//   id: z.string().regex(/^\d+$/, 'Job ID must be a number'),
// })

// Rate limiting configurations
const createCrawlLimiter = rateLimit({
  windowMs: getConfig('RATE_LIMIT', 'WINDOW_MS'),
  max: getConfig('RATE_LIMIT', 'CRAWL_MAX_REQUESTS'),
  message: 'Too many crawl requests from this IP, please try again later.',
  standardHeaders: true, // Return rate limit info in the `RateLimit-*` headers
  legacyHeaders: false, // Disable the `X-RateLimit-*` headers
  // Disable trust proxy validation since we handle it at the app level
  validate: false,
  skip: (req) => {
    // Skip rate limiting if we can't determine the IP (failopen)
    if (!req.ip && !req.ips?.length) {
      log.warning('Unable to determine client IP for rate limiting')
      return true
    }
    return false
  },
})

const globalLimiter = rateLimit({
  windowMs: getConfig('RATE_LIMIT', 'WINDOW_MS'),
  max: getConfig('RATE_LIMIT', 'GLOBAL_MAX_REQUESTS'),
  message: 'Too many requests from this IP, please try again later.',
  validate: false,
  skip: (req) => {
    if (!req.ip && !req.ips?.length) {
      log.warning('Unable to determine client IP for rate limiting')
      return true
    }
    return false
  },
})

// Error handler middleware
const errorHandler = (
  err: Error,
  req: Request,
  res: Response,
  _next: NextFunction
) => {
  const formatted = logError('UnhandledError', err, {
    path: req.path,
    method: req.method,
  })

  const statusCode = err instanceof ScrapixError ? err.statusCode : 500

  res.status(statusCode).json({
    status: 'error',
    error: {
      code: formatted.code,
      message: formatted.message,
      ...(process.env.NODE_ENV !== 'production' && {
        details: formatted.details,
      }),
    },
  })
}

// Request validation middleware
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

class Server {
  taskQueue: TaskQueue
  app: express.Application

  constructor() {
    this.__check_env()
    void this.__initTelemetry()

    this.taskQueue = new TaskQueue()
    this.app = express()

    // Configure Express to trust proxy headers (needed for rate limiting behind load balancers)
    // Set to specific number of proxies or specific IP addresses for production
    // For Koyeb, we typically have 1-2 proxies in the chain
    this.app.set('trust proxy', 2)
    log.info('Express configured to trust 2 proxy hops')

    // Middleware
    this.app.use(express.json({ limit: getConfig('SERVER', 'MAX_BODY_SIZE') }))
    this.app.use(tracingMiddleware()) // Add tracing before other middleware
    this.app.use(metricsMiddleware()) // Add metrics tracking
    this.app.use(globalLimiter as any) // Apply global rate limit to all routes
    this.app.use((req, _res, next) => {
      log.debug('Request received', { method: req.method, path: req.path })
      next()
    })

    // Routes
    this.app.get('/health', (req, res) => this.__health(req, res))
    this.app.post(
      '/crawl',
      createCrawlLimiter as any,
      validateBody(ConfigSchema),
      (req, res) => this.__asyncCrawl(req, res)
    )
    this.app.post(
      '/crawl/async',
      createCrawlLimiter as any,
      validateBody(ConfigSchema),
      (req, res) => this.__asyncCrawl(req, res)
    )
    this.app.post(
      '/crawl/sync',
      createCrawlLimiter as any,
      validateBody(ConfigSchema),
      (req, res) => this.__syncCrawl(req, res)
    )

    this.app.get('/job/:id/status', (req, res) => this.__jobStatus(req, res))
    this.app.get('/job/:id/events', (req, res) => this.__jobEvents(req, res))
    this.app.get('/events/stats', (req, res) => this.__eventStats(req, res))

    this.app.post('/webhook', (req, res) => this.__log_webhook(req, res))

    // Supabase-backed API routes (if configured)
    this.app.use('/configs', configsRouter)
    this.app.use('/runs', runsRouter)

    // Error handler (must be last)
    this.app.use(errorHandler)

    this.app.listen(port, () =>
      log.info(`Crawler app listening on port ${port}!`)
    )
  }

  __check_env() {
    const { REDIS_URL } = process.env
    log.debug('Checking environment variables', { REDIS_URL })
    if (!REDIS_URL) {
      log.warning('REDIS_URL is not set', {
        message: 'Some features may not work properly',
      })
    }
  }

  async __initTelemetry() {
    try {
      await initializeTelemetry()
      log.info('OpenTelemetry initialized successfully')
    } catch (error) {
      log.error('Failed to initialize OpenTelemetry', { error })
    }
  }

  __health(_req: Request, res: Response) {
    res.status(200).send({ status: 'ok', uptime: process.uptime() })
  }

  async __asyncCrawl(req: Request, res: Response) {
    const span = trace.getActiveSpan()
    try {
      const config = req.body // Already validated by middleware

      // Add customer attributes to span
      if (span) {
        const customerAttrs = extractCustomerAttributes(config)
        span.setAttributes(customerAttrs)
        span.setAttribute('operation.type', 'async_crawl')
      }

      // Track business metrics
      businessMetrics.recordCrawlStart(config)

      const job = await this.taskQueue.add(config)
      log.info('Asynchronous crawl task added to queue', {
        config,
        jobId: job.id,
        customerId: extractCustomerId(config),
      })

      if (span) {
        span.setAttribute('job.id', job.id)
      }

      res.status(200).send({
        status: 'ok',
        jobId: job.id,
        indexUid: config.meilisearch_index_uid,
        statusUrl: `/job/${job.id}/status`,
        eventsUrl: `/job/${job.id}/events`,
      })
    } catch (error) {
      if (error instanceof z.ZodError) {
        res.status(400).json({
          status: 'error',
          error: {
            code: ErrorCode.CONFIG_INVALID,
            message: 'Invalid crawler configuration',
            details: error.errors,
          },
        })
      } else {
        const formatted = logError('AsyncCrawl', error)
        res.status(500).json({
          status: 'error',
          error: formatted,
        })
      }
    }
  }

  async __syncCrawl(req: Request, res: Response) {
    const span = trace.getActiveSpan()
    const startTime = Date.now()

    try {
      const config = req.body // Already validated by middleware

      // Add customer attributes to span
      if (span) {
        const customerAttrs = extractCustomerAttributes(config)
        span.setAttributes(customerAttrs)
        span.setAttribute('operation.type', 'sync_crawl')
      }

      log.info('Starting synchronous crawl', {
        config,
        customerId: extractCustomerId(config),
      })

      // Track business metrics
      businessMetrics.recordCrawlStart(config)

      // Create container for dependency injection
      // const container = Container.createDefault(config)
      Container.createDefault(config)

      const sender = new Sender(config)
      await sender.init()

      const crawler = Crawler.create(config.crawler_type, sender, config)

      await Crawler.run(crawler)
      await sender.finish()

      const duration = (Date.now() - startTime) / 1000
      businessMetrics.recordCrawlDuration(config, duration)

      log.info('Synchronous crawl completed', {
        config,
        customerId: extractCustomerId(config),
        durationSeconds: duration,
      })

      res.status(200).send({
        status: 'ok',
        indexUid: config.meilisearch_index_uid,
      })
    } catch (error) {
      if (error instanceof z.ZodError) {
        res.status(400).json({
          status: 'error',
          error: {
            code: ErrorCode.CONFIG_INVALID,
            message: 'Invalid crawler configuration',
            details: error.errors,
          },
        })
      } else {
        const formatted = logError('SyncCrawl', error)
        res.status(500).json({
          status: 'error',
          error: formatted,
        })
      }
    }
  }

  /**
   * Logs the webhook request and sends a response
   *
   * This is an internal endpoint and does not need to be documented.
   */
  async __jobStatus(
    req: Request<{ id: string }>,
    res: Response
  ): Promise<void> {
    try {
      const jobId = req.params.id

      // Validate job ID format
      if (!jobId || !/^\d+$/.test(jobId)) {
        res.status(400).send({ error: 'Invalid job ID format' })
        return
      }
      const job = await this.taskQueue.getJob(jobId)

      if (!job) {
        throw new ScrapixError(
          ErrorCode.JOB_NOT_FOUND,
          `Job with ID ${jobId} not found. It may have expired or been deleted.`,
          { jobId },
          404
        )
      }

      const status = await job.getState()
      const progress = job.progress()

      res.status(200).send({
        jobId: job.id,
        status,
        progress,
        data: job.data,
        createdAt: job.timestamp,
        processedAt: job.processedOn,
        finishedAt: job.finishedOn,
        failedReason: job.failedReason,
      })
    } catch (error) {
      if (error instanceof ScrapixError) {
        res.status(error.statusCode).json(error.toJSON())
      } else {
        const formatted = logError('JobStatus', error, {
          jobId: req.params.id,
        })
        res.status(500).json({
          status: 'error',
          error: formatted,
        })
      }
    }
  }

  __jobEvents(req: Request<{ id: string }>, res: Response) {
    const jobId = req.params.id

    // Validate job ID format
    if (!jobId || !/^\d+$/.test(jobId)) {
      res.status(400).send({ error: 'Invalid job ID format' })
      return
    }

    // Generate unique client ID
    const clientId = `client_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`

    // Register SSE client with event handler
    const eventHandler = this.taskQueue.getEventHandler()
    eventHandler.registerSSEClient(clientId, jobId, res)

    // Send initial job status
    this.taskQueue
      .getJob(jobId)
      .then((job) => {
        if (job) {
          job
            .getState()
            .then((status) => {
              eventHandler.broadcastToJob(jobId, 'job.status', {
                type: 'status',
                status,
                progress: job.progress(),
                jobId,
                timestamp: new Date().toISOString(),
              })
            })
            .catch((error) => {
              log.error('Error getting job state', { error })
              eventHandler.broadcastToJob(jobId, 'job.error', {
                type: 'error',
                message: 'Failed to get job state',
                jobId,
                timestamp: new Date().toISOString(),
              })
            })
        } else {
          eventHandler.broadcastToJob(jobId, 'job.error', {
            type: 'error',
            message: 'Job not found',
            jobId,
            timestamp: new Date().toISOString(),
          })
        }
      })
      .catch((error) => {
        log.error('Error getting job', { error })
        eventHandler.broadcastToJob(jobId, 'job.error', {
          type: 'error',
          message: 'Failed to get job',
          jobId,
          timestamp: new Date().toISOString(),
        })
      })

    // The EventHandler will manage the connection lifecycle
    // Client cleanup is handled automatically when the response closes
  }

  __eventStats(req: Request, res: Response) {
    try {
      const eventHandler = this.taskQueue.getEventHandler()
      const stats = eventHandler.getClientStats()

      res.status(200).json({
        status: 'ok',
        eventStreaming: {
          enabled: true,
          ...stats,
        },
        timestamp: new Date().toISOString(),
      })
    } catch (error) {
      log.error('Error getting event stats', { error })
      res.status(500).json({
        status: 'error',
        error: 'Failed to get event statistics',
      })
    }
  }

  __log_webhook(req: Request, res: Response) {
    log.info('Webhook received', { body: req.body })
    res.status(200).send({ status: 'ok' })
  }
}

// const server = new Server()
new Server()

// Graceful shutdown handling
const gracefulShutdown = async (signal: string) => {
  log.info(`Received ${signal}, shutting down gracefully...`)

  try {
    // Close connection pools
    closeConnectionPools()
    log.info('Connection pools closed')

    // Give ongoing requests a chance to complete
    await new Promise((resolve) => setTimeout(resolve, 5000))

    process.exit(0)
  } catch (error) {
    log.error('Error during shutdown', { error })
    process.exit(1)
  }
}

process.on('SIGINT', () => gracefulShutdown('SIGINT'))
process.on('SIGTERM', () => gracefulShutdown('SIGTERM'))
