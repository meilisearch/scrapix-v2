import { Request, Response, NextFunction } from 'express'
import { trace, SpanKind, SpanStatusCode, context } from '@opentelemetry/api'

// Import telemetry from core - use dynamic import to avoid circular dependency
let telemetry: any

// Initialize telemetry for server
export async function initializeTelemetry() {
  const core = await import('@scrapix/core')
  telemetry = core.telemetry
  await telemetry.initialize('scrapix-server')
}

// Express middleware for tracing HTTP requests
export function tracingMiddleware() {
  return (req: Request, res: Response, next: NextFunction) => {
    if (!telemetry) {
      // Telemetry not initialized yet, skip tracing
      return next()
    }
    
    const tracer = telemetry.tracer
    
    const requestSpan = (() => {
      return tracer.startSpan(
        `${req.method} ${req.path}`,
        {
          kind: SpanKind.SERVER,
          attributes: {
            'http.method': req.method,
            'http.url': req.url,
            'http.target': req.path,
            'http.host': req.hostname,
            'http.scheme': req.protocol,
            'http.user_agent': req.get('user-agent'),
            'http.remote_addr': req.ip,
          },
        }
      )
    })()

    // Store span in request for later use
    (req as any).span = requestSpan

    // Capture response details
    const originalSend = res.send
    res.send = function(data: any) {
      requestSpan.setAttributes({
        'http.status_code': res.statusCode,
        'http.response.size': Buffer.byteLength(JSON.stringify(data)),
      })

      if (res.statusCode >= 400) {
        requestSpan.setStatus({
          code: SpanStatusCode.ERROR,
          message: `HTTP ${res.statusCode}`,
        })
      } else {
        requestSpan.setStatus({ code: SpanStatusCode.OK })
      }

      requestSpan.end()
      return originalSend.call(this, data)
    }

    // Handle errors
    const originalNext = next
    next = ((err?: any) => {
      if (err) {
        requestSpan.recordException(err)
        requestSpan.setStatus({
          code: SpanStatusCode.ERROR,
          message: err.message,
        })
      }
      originalNext(err)
    }) as NextFunction

    // Continue with request in span context
    context.with(trace.setSpan(context.active(), requestSpan), () => {
      next()
    })
  }
}

// Helper to get current span from request
export function getRequestSpan(req: Request) {
  return (req as any).span
}

// Metrics for server endpoints
class ServerMetrics {
  private requestCounter: any
  private requestDuration: any
  private activeRequests: any

  constructor() {
    // Meters will be initialized lazily when first used
  }

  private ensureInitialized() {
    if (!this.requestCounter && telemetry) {
      this.requestCounter = telemetry.meter.createCounter('http.server.requests', {
        description: 'Total HTTP requests',
      })

      this.requestDuration = telemetry.meter.createHistogram('http.server.duration', {
        description: 'HTTP request duration',
        unit: 'ms',
      })

      this.activeRequests = telemetry.meter.createUpDownCounter('http.server.active_requests', {
        description: 'Number of active HTTP requests',
      })
    }
  }

  recordRequest(method: string, path: string, statusCode: number, duration: number) {
    this.ensureInitialized()
    if (!this.requestCounter) return

    const attributes = {
      'http.method': method,
      'http.route': path,
      'http.status_code': statusCode,
      'http.status_class': `${Math.floor(statusCode / 100)}xx`,
    }

    this.requestCounter.add(1, attributes)
    this.requestDuration.record(duration, attributes)
  }

  incrementActiveRequests() {
    this.ensureInitialized()
    if (this.activeRequests) {
      this.activeRequests.add(1)
    }
  }

  decrementActiveRequests() {
    this.ensureInitialized()
    if (this.activeRequests) {
      this.activeRequests.add(-1)
    }
  }
}

export const serverMetrics = new ServerMetrics()

// Middleware to track request metrics
export function metricsMiddleware() {
  return (req: Request, res: Response, next: NextFunction) => {
    const startTime = Date.now()
    serverMetrics.incrementActiveRequests()

    // Capture when response finishes
    res.on('finish', () => {
      const duration = Date.now() - startTime
      serverMetrics.recordRequest(req.method, req.route?.path || req.path, res.statusCode, duration)
      serverMetrics.decrementActiveRequests()
    })

    next()
  }
}