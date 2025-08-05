import { RequestQueue, Router, ProxyConfiguration } from 'crawlee'
import { minimatch } from 'minimatch'
import { createScraper } from '../scrapers'
import { Sender } from '../sender'
import { Config, Scraper, CrawlerType } from '../types'
import { Log } from 'crawlee'
import * as cheerio from 'cheerio'
import { telemetry, businessMetrics } from '../telemetry'
import { SpanStatusCode } from '@opentelemetry/api'

const log = new Log({ prefix: 'BaseCrawler' })

export abstract class BaseCrawler {
  sender: Sender
  config: Config
  urls: string[]
  scraper: Scraper
  nb_page_crawled = 0
  nb_page_indexed = 0
  crawlerType: CrawlerType
  proxyConfiguration?: ProxyConfiguration

  constructor(sender: Sender, config: Config) {
    this.sender = sender
    this.config = config
    this.urls = config.start_urls
    this.crawlerType = config.crawler_type || 'cheerio'

    // Initialize proxy configuration if provided
    if (config.proxy_configuration) {
      this.proxyConfiguration = new ProxyConfiguration(
        config.proxy_configuration
      )
    }

    this.scraper = createScraper(this.sender, this.config)
  }

  abstract createRouter(): Router<any>
  abstract getCrawlerOptions(
    requestQueue: RequestQueue,
    router: Router<any>
  ): any
  abstract createCrawlerInstance(options: any): any

  // Add this new method
  async defaultHandler(context: any): Promise<void> {
    log.info('Default handler called', { url: context.request.url })
    await this.handlePage(context)
  }

  // New method to handle the common logic
  protected async handlePage(context: any): Promise<void> {
    const { request, enqueueLinks } = context
    const url = request.loadedUrl
    
    // Start telemetry span for page processing
    const span = telemetry.startSpan('crawler.page.process', this.config, {
      'page.url': url,
      'crawler.type': this.crawlerType,
      'page.depth': request.userData?.depth || 0,
    })
    
    this.nb_page_crawled++
    log.info('Processing page', { 
      url,
      crawledCount: this.nb_page_crawled,
      indexedCount: this.nb_page_indexed
    })

    const crawled_globs = this.__generate_globs(this.config.start_urls)
    const excluded_crawled_globs = this.__generate_globs(
      this.config.urls_to_exclude || []
    )
    const indexed_globs = this.__generate_globs(
      this.config.urls_to_index || this.urls
    )
    const excluded_indexed_globs = this.__generate_globs(
      this.config.urls_to_not_index || []
    )

    try {
      if (request.loadedUrl) {
        if (
          this.__match_globs(request.loadedUrl, indexed_globs) &&
          !this.__match_globs(request.loadedUrl, excluded_indexed_globs)
        ) {
          let $: cheerio.CheerioAPI

          try {
            let pageContent: string | undefined
            
            if (this.crawlerType === 'puppeteer') {
              pageContent = await context.page.content()
              $ = cheerio.load(pageContent || '')
            } else {
              $ = context.$
            }

            if (!$) {
              log.error('Cheerio instance is undefined', {
                url: request.loadedUrl,
              })
              span.setAttributes({
                'page.error': 'cheerio_undefined',
                'page.indexed': false,
              })
              span.setStatus({
                code: SpanStatusCode.ERROR,
                message: 'Cheerio instance undefined',
              })
              return
            }

            // Check for 404 before incrementing counter and scraping
            if (this.__is404Page($)) {
              log.debug('404 page detected, skipping', {
                url: request.loadedUrl,
              })
              span.setAttributes({
                'page.status': 404,
                'page.indexed': false,
                'page.skip_reason': 'not_found',
              })
              span.setStatus({ code: SpanStatusCode.OK })
              return
            }

            this.nb_page_indexed++
            
            // Track page as successfully crawled
            businessMetrics.recordPageCrawled(this.config, true)
            
            log.debug('Starting scraper.get', { url: request.loadedUrl })
            await this.scraper.get(request.loadedUrl, $)
            log.debug('Completed scraper.get', { url: request.loadedUrl })
            
            span.setAttributes({
              'page.indexed': true,
              'page.success': true,
            })
          
          // Clean up to help garbage collection
          if (pageContent) {
            pageContent = undefined
          }
          
          // For Puppeteer, explicitly clear the $ reference
          if (this.crawlerType === 'puppeteer') {
            $ = null as any
          }
          } catch (error) {
            log.error('Error processing page', {
              url: request.loadedUrl,
              error: error instanceof Error ? error.message : String(error),
            })
            
            // Track failed page crawl
            businessMetrics.recordPageCrawled(this.config, false)
            
            span.recordException(error as Error)
            span.setAttributes({
              'page.indexed': false,
              'page.error': (error as Error).message,
            })
            span.setStatus({
              code: SpanStatusCode.ERROR,
              message: (error as Error).message,
            })
          } finally {
            // Additional cleanup for Puppeteer pages
            if (this.crawlerType === 'puppeteer' && context.page) {
              try {
                // Clear any event listeners that might have been added
                await context.page.removeAllListeners()
              } catch (e) {
                // Ignore cleanup errors
              }
            }
          }
        } else {
          // URL not indexed due to filters
          span.setAttributes({
            'page.indexed': false,
            'page.skip_reason': 'url_filter',
          })
        }
      }

      await enqueueLinks({
        globs: crawled_globs,
        exclude: excluded_crawled_globs,
        transformRequestFunction: (req: any) => {
          if (this.__is_file_url(req.url)) {
            return false
          }
          const urlObject = new URL(req.url)
          urlObject.search = ''
          urlObject.hash = ''
          req.url = urlObject.toString()
          return req
        },
      })
      
      // Set final span attributes
      span.setAttributes({
        'page.crawled_count': this.nb_page_crawled,
        'page.indexed_count': this.nb_page_indexed,
      })
      
      // Set success status
      span.setStatus({ code: SpanStatusCode.OK })
    } catch (error) {
      // Catch any unexpected errors
      span.recordException(error as Error)
      span.setStatus({
        code: SpanStatusCode.ERROR,
        message: (error as Error).message,
      })
      throw error
    } finally {
      span.end()
    }
  }

  protected __generate_globs(urls: string[]): string[] {
    return urls.flatMap((url) => {
      if (url.endsWith('/')) {
        return [url, url + '**']
      }
      return [url, url + '/**']
    })
  }

  protected __match_globs(url: string, globs: string[]): boolean {
    return globs.some((glob) => minimatch(url, glob))
  }

  protected __is_file_url(url: string): boolean {
    // Use a Set for O(1) lookup performance and automatic deduplication
    const fileExtensions = new Set([
      // Data formats
      '.json',
      '.csv',
      '.yaml',
      '.yml',
      '.xml',
      '.sql',
      '.db',
      '.sqlite',
      
      // Documents
      '.md',
      '.markdown',
      '.txt',
      '.rtf',
      '.doc',
      '.docx',
      '.xls',
      '.xlsx',
      '.ppt',
      '.pptx',
      
      // Configuration
      '.ini',
      '.config',
      '.log',
      
      // Archives
      '.zip',
      '.rar',
      '.tar',
      '.gz',
      '.tgz',
      '.7z',
      '.bz2',
      
      // Executables
      '.exe',
      '.bin',
      '.apk',
      '.ipa',
      '.dmg',
      '.iso',
      
      // Images
      '.jpg',
      '.jpeg',
      '.png',
      '.gif',
      '.svg',
      
      // Web assets
      '.css',
      '.js',
      
      // Media
      '.mp3',
      '.wav',
      '.mp4',
      '.avi',
      '.mkv',
      '.mov',
      '.flv',
      '.wmv',
      '.m4v',
      '.ogg',
      '.mpg',
      '.mpeg',
      '.swf',
    ])
    
    // Extract the extension from the URL (ignoring query parameters)
    const urlPath = url.split('?')[0].toLowerCase()
    return Array.from(fileExtensions).some((extension) => urlPath.endsWith(extension))
  }

  protected __is404Page($: cheerio.CheerioAPI): boolean {
    // Use custom selectors if provided, otherwise use defaults
    const customSelectors = this.config.not_found_selectors

    if (customSelectors && customSelectors.length > 0) {
      return customSelectors.some((selector) => $(selector).length > 0)
    }

    // Default selectors if no custom ones provided
    const defaultSelectors = [
      // Basic text content selectors
      'h1:contains("404")',
      'h1:contains("Page Not Found")',
      'title:contains("404")',

      // Multiple elements check
      'div:contains("404"), span:contains("404")',

      // Class-based selectors
      '.error-404',
      '.not-found',
      '#error-page',

      // Attribute selectors
      '[data-error="404"]',
      '[data-page-type="404"]',
    ]

    // Common error texts to check in body
    const commonErrorTexts = [
      'page not found',
      "page doesn't exist",
      'page could not be found',
      '404 error',
    ]

    // Check default selectors
    const hasErrorSelector = defaultSelectors.some(
      (selector) => $(selector).length > 0
    )

    // Check text content
    const bodyText = $('body')
      .clone()
      .find('script')
      .remove()
      .end()
      .text()
      .toLowerCase()

    const hasErrorText = commonErrorTexts.some((text) =>
      bodyText.includes(text)
    )

    return hasErrorSelector || hasErrorText
  }
}
