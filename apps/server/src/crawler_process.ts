import {
  Sender,
  Crawler,
  Config,
  getEventBus,
  CrawlerEventMap,
  CrawlerEvents,
} from '@scrapix/core'
import * as fs from 'fs'

async function startCrawling(config: Config) {
  // Import Configuration from crawlee
  const { Configuration } = await import('crawlee')

  // Disable storage persistence for Docker environment
  Configuration.getGlobalConfig().set('persistStorage', false)
  Configuration.getGlobalConfig().set('persistStateIntervalMillis', 0)

  // Use memory storage instead of disk
  const storageDir = process.env.CRAWLEE_STORAGE_DIR || '/tmp/crawlee-storage'

  // Still create directories as fallback
  try {
    fs.mkdirSync(storageDir, { recursive: true, mode: 0o777 })
  } catch (error) {
    console.error('Error creating storage directory:', error)
  }

  process.env.CRAWLEE_STORAGE_DIR = storageDir
  console.log('Storage directory set to:', storageDir, '(persistence disabled)')

  // Set up event forwarding via IPC
  const eventBus = getEventBus()
  setupEventForwarding(eventBus)

  // Emit crawler started event
  eventBus.emitCrawlerStarted({
    config,
    startUrls: config.start_urls,
    crawlerType: config.crawler_type || 'cheerio',
    featuresEnabled: Object.keys(config.features || {}).filter(
      (key) => (config.features as any)?.[key]?.activated
    ),
    indexUid: config.meilisearch_index_uid,
  })

  const sender = new Sender(config)
  await sender.init()

  const crawler = Crawler.create(
    config.crawler_type || 'cheerio',
    sender,
    config
  )

  try {
    const crawlStartTime = Date.now()
    await Crawler.run(crawler)
    await sender.finish()
    const _crawlDuration = Date.now() - crawlStartTime

    // Emit crawler completed event
    const metrics = eventBus.getMetrics()
    eventBus.emitCrawlerCompleted({
      totalCrawled: metrics.totalCrawled,
      totalIndexed: metrics.totalIndexed,
      totalDocumentsSent: metrics.totalDocumentsSent,
      success: true,
      memoryUsage: process.memoryUsage(),
      stats: {
        avgPageTime: metrics.avgPageTime,
        pagesPerMinute: metrics.currentRate,
        successRate: metrics.successRate,
        totalRetries: metrics.totalRetries,
        totalErrors: metrics.totalErrors,
      },
    })
  } catch (error) {
    // Emit crawler completed event with error
    const metrics = eventBus.getMetrics()
    eventBus.emitCrawlerCompleted({
      totalCrawled: metrics.totalCrawled,
      totalIndexed: metrics.totalIndexed,
      totalDocumentsSent: metrics.totalDocumentsSent,
      success: false,
      error: error instanceof Error ? error.message : String(error),
      memoryUsage: process.memoryUsage(),
      stats: {
        avgPageTime: metrics.avgPageTime,
        pagesPerMinute: metrics.currentRate,
        successRate: metrics.successRate,
        totalRetries: metrics.totalRetries,
        totalErrors: metrics.totalErrors,
      },
    })
    throw error
  }
}

/**
 * Set up event forwarding from EventBus to parent process via IPC
 */
function setupEventForwarding(
  eventBus: typeof getEventBus extends () => infer T ? T : never
): void {
  // Forward all crawler events to parent process
  Object.values(CrawlerEvents).forEach((eventName) => {
    eventBus.on(eventName as keyof CrawlerEventMap, (eventData) => {
      if (process.send) {
        process.send({
          type: 'crawler.event',
          eventName,
          eventData,
          timestamp: new Date().toISOString(),
        })
      }
    })
  })

  // Forward batch processed events
  eventBus.on('batch.processed' as any, (batchData) => {
    if (process.send) {
      process.send({
        type: 'batch.processed',
        batchData,
        timestamp: new Date().toISOString(),
      })
    }
  })

  // Set up progress updates every 5 seconds
  const progressInterval = setInterval(() => {
    const metrics = eventBus.getMetrics()
    const memoryUsage = process.memoryUsage()

    eventBus.emitProgressUpdate({
      progress: {
        crawled: metrics.totalCrawled,
        indexed: metrics.totalIndexed,
        documentsSent: metrics.totalDocumentsSent,
        errors: metrics.totalErrors,
      },
      completionPercentage: metrics.completionPercentage,
      estimatedTimeRemaining: metrics.estimatedTimeRemaining,
      currentRate: metrics.currentRate,
      avgPageTime: metrics.avgPageTime,
      memoryUsage,
    })
  }, 5000)

  // Clean up interval when process exits
  process.on('exit', () => {
    clearInterval(progressInterval)
  })

  process.on('SIGINT', () => {
    clearInterval(progressInterval)
    process.exit(0)
  })

  process.on('SIGTERM', () => {
    clearInterval(progressInterval)
    process.exit(0)
  })
}

// Listen for messages from the parent thread
process.on('message', async (message: Config) => {
  await startCrawling(message)
  if (process.send) {
    process.send('Crawling finished')
  }
})
