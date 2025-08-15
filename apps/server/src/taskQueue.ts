import Queue, { Job, DoneCallback } from 'bull'
import { fork } from 'child_process'
import { join } from 'path'
import {
  Config,
  initMeilisearchClient,
  queueMetrics,
  JobTelemetry,
  extractCustomerId,
} from '@scrapix/core'
import { Log } from 'crawlee'
import { getEventHandler } from './events/event-handler'

const log = new Log({ prefix: 'CrawlTaskQueue' })

export class TaskQueue {
  queue: Queue.Queue
  private eventHandler = getEventHandler()

  constructor() {
    log.info('Initializing CrawlTaskQueue', {
      redisUrl: process.env.REDIS_URL,
    })

    const queueName = 'crawling'

    try {
      // Initialize queue with Redis URL if available
      if (process.env.REDIS_URL) {
        const redisUrl = process.env.REDIS_URL
        const isTLS = redisUrl.startsWith('rediss://')

        const redisOptions: any = {
          connectTimeout: 10000,
          retryStrategy: (times: number) => {
            const delay = Math.min(times * 50, 2000)
            log.warning(
              `Redis connection attempt ${times}, retrying in ${delay}ms`
            )
            return delay
          },
          lazyConnect: false,
        }

        // Add TLS configuration for rediss:// URLs
        if (isTLS) {
          redisOptions.tls = {
            rejectUnauthorized: false,
          }
        }

        this.queue = new Queue(queueName, redisUrl, { redis: redisOptions })
      } else {
        this.queue = new Queue(queueName)
      }

      if (process.env.REDIS_URL) {
        // Set up Redis error handlers
        const client = this.queue.client
        client.on('error', (error) => {
          log.error('Redis client error', { error: error.message })
        })
        client.on('connect', () => {
          log.info('Redis client connected')
        })
        client.on('ready', () => {
          log.info('Redis client ready')
        })

        // Set up queue event handlers
        void this.queue.process(this.__process.bind(this))

        const eventHandlers = {
          added: this.__jobAdded,
          completed: this.__jobCompleted,
          failed: this.__jobFailed,
          active: this.__jobActive,
          wait: this.__jobWaiting,
          delayed: this.__jobDelayed,
        }

        // Bind all event handlers
        Object.entries(eventHandlers).forEach(([event, handler]) => {
          this.queue.on(event, handler.bind(this))
        })

        // Set up queue metrics callbacks
        queueMetrics.setQueueDepthCallback(async () => {
          const count = await this.queue.getWaitingCount()
          return count
        })

        queueMetrics.setActiveJobsCallback(async () => {
          const count = await this.queue.getActiveCount()
          return count
        })
      }
    } catch (error) {
      // Fallback to local queue if Redis connection fails
      this.queue = new Queue(queueName)
      log.error('Error while initializing CrawlTaskQueue', {
        error,
        message: (error as Error).message,
      })
    }
  }

  async add(data: Config) {
    log.debug('Adding task to queue', {
      config: data,
      customerId: extractCustomerId(data),
    })
    queueMetrics.recordJobAdded(data)
    return await this.queue.add(data)
  }

  async getJob(jobId: string) {
    return await this.queue.getJob(jobId)
  }

  /**
   * Get the event handler instance for SSE connections
   */
  getEventHandler() {
    return this.eventHandler
  }

  async __process(job: Job<Config>, done: DoneCallback) {
    log.debug('Processing job', {
      jobId: job.id,
      customerId: extractCustomerId(job.data),
    })

    // Wrap job processing with telemetry
    JobTelemetry.processWithTelemetry(job, async () => {
      return new Promise((resolve, reject) => {
        const crawlerPath = join(__dirname, 'crawler_process.js')
        const childProcess = fork(crawlerPath)

        childProcess.send(job.data)

        // Handle IPC messages from crawler process
        childProcess.on('message', (message: any) => {
          if (typeof message === 'object' && message.type) {
            switch (message.type) {
              case 'crawler.event':
                log.debug('Received crawler event via IPC', {
                  eventName: message.eventName,
                  jobId: job.id,
                })
                this.eventHandler.handleCrawlerEvent(
                  message.eventName,
                  message.eventData,
                  job.id.toString()
                )
                break

              case 'batch.processed':
                log.debug('Received batch processed event via IPC', {
                  batchId: message.batchData.batchId,
                  eventCount: message.batchData.events?.length || 0,
                  jobId: job.id,
                })
                this.eventHandler.handleBatchProcessed(
                  message.batchData,
                  job.id.toString()
                )
                break

              default:
                log.debug('Received unknown message type via IPC', {
                  type: message.type,
                  jobId: job.id,
                })
            }
          } else {
            // Legacy message handling for completion
            log.info('Crawler process message', { message, jobId: job.id })
            resolve(message)
          }
        })

        childProcess.on('error', (error: Error) => {
          log.error('Crawler process error', { error, jobId: job.id })

          // Notify event handler of job failure
          this.eventHandler.broadcastToJob(job.id.toString(), 'job.error', {
            jobId: job.id,
            error: error.message,
            timestamp: new Date().toISOString(),
          })

          reject(error)
        })

        childProcess.on('exit', (code) => {
          if (code !== 0) {
            log.error('Crawler process exited with non-zero code', {
              code,
              jobId: job.id,
            })

            // Notify event handler of job failure
            this.eventHandler.broadcastToJob(job.id.toString(), 'job.failed', {
              jobId: job.id,
              exitCode: code,
              timestamp: new Date().toISOString(),
            })

            reject(new Error(`Crawler process exited with code ${code}`))
          } else {
            // Successful completion
            log.info('Crawler process completed successfully', {
              jobId: job.id,
            })

            // Notify event handler of job completion
            this.eventHandler.broadcastToJob(
              job.id.toString(),
              'job.completed',
              {
                jobId: job.id,
                timestamp: new Date().toISOString(),
              }
            )

            resolve('Crawling finished')
          }
        })
      })
    })
      .then(() => done())
      .catch((error) => done(error))
  }

  __jobAdded(job: Job) {
    log.debug('Job added to queue', { jobId: job.id })
  }

  __jobCompleted(job: Job) {
    log.debug('Job completed', { jobId: job.id })
  }

  async __jobFailed(job: Job<Config>) {
    log.error('Job failed', { jobId: job.id })
    //Create a Meilisearch client
    const client = initMeilisearchClient({
      host: job.data.meilisearch_url,
      apiKey: job.data.meilisearch_api_key,
      clientAgents: job.data.user_agents,
    })

    //check if the tmp index exists
    const tmp_index_uid = job.data.meilisearch_index_uid + '_crawler_tmp'
    try {
      const index = await client.getIndex(tmp_index_uid)
      if (index) {
        const task = await client.deleteIndex(tmp_index_uid)
        await client.waitForTask(task.taskUid)
      }
    } catch (e) {
      log.error('Error while deleting tmp index', { error: e })
    }
  }

  __jobActive(job: Job) {
    log.debug('Job became active', { jobId: job.id })
  }

  __jobWaiting(job: Job) {
    log.debug('Job is waiting', { jobId: job.id })
  }

  __jobDelayed(job: Job) {
    log.debug('Job is delayed', { jobId: job.id })
  }
}
