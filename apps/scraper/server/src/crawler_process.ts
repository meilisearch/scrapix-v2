import { Sender, Crawler, Config } from '@scrapix/core'
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

  const sender = new Sender(config)
  await sender.init()

  const crawler = Crawler.create(config.crawler_type || 'cheerio', sender, config)

  await Crawler.run(crawler)
  await sender.finish()
}

// Listen for messages from the parent thread
process.on('message', async (message: Config) => {
  await startCrawling(message)
  if (process.send) {
    process.send('Crawling finished')
  }
})
