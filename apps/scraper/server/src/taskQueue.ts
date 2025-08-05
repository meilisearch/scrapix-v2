import Queue, { Job, DoneCallback } from 'bull'
import { fork } from 'child_process'
import { join } from 'path'
import { Config, initMeilisearchClient, queueMetrics, JobTelemetry, extractCustomerId } from '@scrapix/core'
import { Log } from 'crawlee'

const log = new Log({ prefix: 'CrawlTaskQueue' })

export class TaskQueue {
  queue: Queue.Queue

  constructor() {
    log.info('Initializing CrawlTaskQueue', {
      redisUrl: process.env.REDIS_URL,
    })

    const queueName = 'crawling'

    try {
      // Initialize queue with Redis URL if available
      this.queue = process.env.REDIS_URL
        ? new Queue(queueName, process.env.REDIS_URL)
        : new Queue(queueName)

      if (process.env.REDIS_URL) {
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
    log.debug('Adding task to queue', { config: data, customerId: extractCustomerId(data) })
    queueMetrics.recordJobAdded(data)
    return await this.queue.add(data)
  }

  async getJob(jobId: string) {
    return await this.queue.getJob(jobId)
  }

  async __process(job: Job<Config>, done: DoneCallback) {
    log.debug('Processing job', { jobId: job.id, customerId: extractCustomerId(job.data) })
    
    // Wrap job processing with telemetry
    JobTelemetry.processWithTelemetry(job, async () => {
      return new Promise((resolve, reject) => {
        const crawlerPath = join(__dirname, 'crawler_process.js')
        const childProcess = fork(crawlerPath)
        
        childProcess.send(job.data)
        
        childProcess.on('message', (message) => {
          log.info('Crawler process message', { message, jobId: job.id })
          resolve(message)
        })
        
        childProcess.on('error', (error: Error) => {
          log.error('Crawler process error', { error, jobId: job.id })
          reject(error)
        })
        
        childProcess.on('exit', (code) => {
          if (code !== 0) {
            log.error('Crawler process exited with non-zero code', { code, jobId: job.id })
            reject(new Error(`Crawler process exited with code ${code}`))
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
