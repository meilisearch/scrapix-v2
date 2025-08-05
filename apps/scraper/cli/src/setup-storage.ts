import { Configuration } from 'crawlee'
import * as fs from 'fs'
import * as path from 'path'
import * as os from 'os'

// Configure Crawlee to use a temporary storage directory
const storageDir = path.join(os.tmpdir(), 'scrapix-cli-storage-' + Date.now())

// Create all necessary directories upfront
const dirs = [
  storageDir,
  path.join(storageDir, 'request_queues'),
  path.join(storageDir, 'request_queues', 'default'),
  path.join(storageDir, 'key_value_stores'),
  path.join(storageDir, 'key_value_stores', 'default'),
  path.join(storageDir, 'datasets'),
  path.join(storageDir, 'datasets', 'default'),
]

for (const dir of dirs) {
  try {
    fs.mkdirSync(dir, { recursive: true, mode: 0o777 })
    console.log('Created directory:', dir)
  } catch (error) {
    console.error('Failed to create directory:', dir, error)
  }
}

// Also set environment variable for compatibility
process.env.CRAWLEE_STORAGE_DIR = storageDir

// Configure Crawlee to use simpler storage
Configuration.getGlobalConfig().set('persistStorage', false)
Configuration.getGlobalConfig().set('purgeOnStart', true)

// Enable debug logging
process.env.CRAWLEE_LOG_LEVEL = 'INFO'

console.log('Crawlee storage configured:', storageDir)

export { storageDir }