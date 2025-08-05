// Mark as CLI usage
process.env.SCRAPIX_CLI = 'true'

// Setup storage before any other imports
import './setup-storage'

import * as dotenv from 'dotenv'
import { Log } from 'crawlee'

const log = new Log({ prefix: 'Scraper CLI' })

// Store the original working directory where the command was run
const originalCwd = process.env.INIT_CWD || process.cwd()

// Load .env file first from project root
dotenv.config({ path: ['../../../.env.local', '../../../.env'] })

import fs from 'fs'
import path from 'path'
import yargs from 'yargs'
import { hideBin } from 'yargs/helpers'
import { Sender, Crawler, Config, ConfigSchema } from '@scrapix/core';

function getConfig({
  configPath,
  config,
}: {
  configPath?: string
  config?: string
}): Config {
  let parsedConfig: unknown

  if (configPath) {
    // Resolve path relative to original working directory
    const resolvedPath = path.isAbsolute(configPath) 
      ? configPath 
      : path.resolve(originalCwd, configPath)
    
    parsedConfig = JSON.parse(
      fs.readFileSync(resolvedPath, { encoding: 'utf-8' })
    )
  } else if (config) {
    parsedConfig = JSON.parse(config)
  } else {
    throw new Error('Please provide either --config or --configPath')
  }

  // Validate config against schema
  const validatedConfig = ConfigSchema.parse(parsedConfig)
  return validatedConfig
}

;(async () => {
  // Parse command line arguments and get a configuration file path
  const argv = await yargs(hideBin(process.argv))
    .option('config', {
      alias: 'c',
      describe: 'configuration',
      type: 'string',
    })
    .option('configPath', {
      alias: 'p',
      describe: 'Path to configuration file',
      type: 'string',
    })
    .option('browserPath', {
      alias: 'b',
      describe: 'Path to browser binary',
      type: 'string',
    })
    .check((argv) => {
      if (argv.config && argv.configPath) {
        throw new Error(
          'You can only use either --config or --configPath, not both.'
        )
      } else if (!argv.config && !argv.configPath) {
        throw new Error('You must provide one of --config or --configPath.')
      }
      return true
    }).argv

  log.info('Starting scraper', {
    configPath: argv.configPath,
  })

  const config = getConfig(argv)
  const launchOptions = argv.browserPath
    ? { executablePath: argv.browserPath }
    : {}

  const sender = new Sender(config)
  await sender.init()

  console.log('DEBUG: Creating crawler...')
  const crawler = await Crawler.create(
    config.crawler_type || 'cheerio',
    sender,
    config,
    config.launch_options || launchOptions
  )
  console.log('DEBUG: Crawler created, about to run...')

  try {
    await Crawler.run(crawler)
  } catch (error) {
    console.error('DEBUG: Crawler.run failed:', error)
    throw error
  }
  
  console.log('DEBUG: Crawler.run completed, finishing sender...')
  await sender.finish()
  console.log('DEBUG: All done!')
})()
