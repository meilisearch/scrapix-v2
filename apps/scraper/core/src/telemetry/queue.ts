import { Job } from 'bull'
import { telemetry, businessMetrics, extractCustomerAttributes } from './index'
import { Config } from '../types'
import { SpanKind, SpanStatusCode, context, trace } from '@opentelemetry/api'

// Queue metrics
class QueueMetrics {
  private jobsAdded = telemetry.meter.createCounter('scraper.queue.jobs.added', {
    description: 'Jobs added to queue',
  })

  private jobsCompleted = telemetry.meter.createCounter('scraper.queue.jobs.completed', {
    description: 'Jobs completed successfully',
  })

  private jobsFailed = telemetry.meter.createCounter('scraper.queue.jobs.failed', {
    description: 'Jobs that failed',
  })

  private jobDuration = telemetry.meter.createHistogram('scraper.queue.job.duration', {
    description: 'Job processing duration',
    unit: 'ms',
  })

  private queueDepth = telemetry.meter.createObservableGauge('scraper.queue.depth', {
    description: 'Current queue depth',
  })

  private activeJobs = telemetry.meter.createObservableGauge('scraper.queue.jobs.active', {
    description: 'Currently active jobs',
  })

  setQueueDepthCallback(callback: () => Promise<number>) {
    this.queueDepth.addCallback(async (result) => {
      result.observe(await callback())
    })
  }

  setActiveJobsCallback(callback: () => Promise<number>) {
    this.activeJobs.addCallback(async (result) => {
      result.observe(await callback())
    })
  }

  recordJobAdded(config: Config) {
    const attrs = extractCustomerAttributes(config)
    this.jobsAdded.add(1, attrs)
  }

  recordJobCompleted(config: Config, durationMs: number) {
    const attrs = extractCustomerAttributes(config)
    this.jobsCompleted.add(1, attrs)
    this.jobDuration.record(durationMs, attrs)
  }

  recordJobFailed(config: Config, error: string) {
    const attrs = {
      ...extractCustomerAttributes(config),
      'error.type': error,
    }
    this.jobsFailed.add(1, attrs)
  }
}

export const queueMetrics = new QueueMetrics()

// Job processing telemetry wrapper
export class JobTelemetry {
  /**
   * Wraps job processing with telemetry
   */
  static async processWithTelemetry<T>(
    job: Job<Config>,
    processor: () => Promise<T>
  ): Promise<T> {
    const config = job.data
    const span = telemetry.tracer.startSpan(
      'queue.job.process',
      {
        kind: SpanKind.CONSUMER,
        attributes: {
          'job.id': job.id?.toString(),
          'job.name': job.name,
          'job.queue': job.queue.name,
          'job.attempts': job.attemptsMade,
          'job.timestamp': job.timestamp,
          ...extractCustomerAttributes(config),
        },
      }
    )

    const startTime = Date.now()
    businessMetrics.recordCrawlStart(config)

    try {
      const result = await context.with(
        trace.setSpan(context.active(), span),
        () => processor()
      )

      const duration = Date.now() - startTime
      span.setStatus({ code: SpanStatusCode.OK })
      span.setAttributes({
        'job.duration_ms': duration,
        'job.success': true,
      })

      queueMetrics.recordJobCompleted(config, duration)
      businessMetrics.recordCrawlDuration(config, duration / 1000)

      return result
    } catch (error) {
      const errorMessage = (error as Error).message
      span.recordException(error as Error)
      span.setStatus({
        code: SpanStatusCode.ERROR,
        message: errorMessage,
      })
      span.setAttributes({
        'job.duration_ms': Date.now() - startTime,
        'job.success': false,
        'job.error': errorMessage,
      })

      queueMetrics.recordJobFailed(config, errorMessage)
      throw error
    } finally {
      span.end()
    }
  }

  /**
   * Create span for job status checks
   */
  static trackJobStatus(jobId: string, status: string) {
    const span = telemetry.tracer.startSpan('queue.job.status', {
      kind: SpanKind.INTERNAL,
      attributes: {
        'job.id': jobId,
        'job.status': status,
      },
    })
    span.end()
    return status
  }

  /**
   * Track job progress updates
   */
  static trackJobProgress(job: Job, progress: number) {
    telemetry.tracer.startActiveSpan(
      'queue.job.progress',
      {
        attributes: {
          'job.id': job.id?.toString(),
          'job.progress': progress,
        },
      },
      (span) => {
        span.addEvent('progress_update', {
          progress,
          timestamp: Date.now(),
        })
        span.end()
      }
    )
  }
}