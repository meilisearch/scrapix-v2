import { Sender, Crawler, Config } from '@scrapix/core'
import * as path from 'path'
import * as fs from 'fs'

async function startCrawling(config: Config) {
  // Set up Crawlee storage directory
  const storageDir = process.env.CRAWLEE_STORAGE_DIR || path.join(__dirname, '..', 'storage')

  // Ensure storage directories exist
  const dirs = [
    path.join(storageDir, 'request_queues', 'default'),
    path.join(storageDir, 'key_value_stores', 'default'),
    path.join(storageDir, 'datasets', 'default'),
  ]

  for (const dir of dirs) {
    fs.mkdirSync(dir, { recursive: true })
  }

  // Set the environment variable for Crawlee to use
  process.env.CRAWLEE_STORAGE_DIR = storageDir

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
