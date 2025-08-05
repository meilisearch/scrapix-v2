import {
  createCheerioRouter,
  CheerioCrawler as CrawleeCheerioCrawler,
  CheerioCrawlerOptions,
  CheerioHook,
  CheerioCrawlingContext,
  Router,
  RequestQueue,
} from 'crawlee'
import { BaseCrawler } from './base'
import { Sender } from '../sender'
import { Config } from '../types'

export class CheerioCrawler extends BaseCrawler {
  constructor(sender: Sender, config: Config) {
    super(sender, config)
  }

  createRouter(): Router<CheerioCrawlingContext> {
    return createCheerioRouter()
  }

  getCrawlerOptions(
    requestQueue: RequestQueue,
    router: Router<CheerioCrawlingContext>
  ): CheerioCrawlerOptions {
    const preNavigationHooks: CheerioHook[] = this.config
      .additional_request_headers
      ? [
          (crawlingContext) => {
            const { request } = crawlingContext
            request.headers = {
              ...request.headers,
              ...this.config.additional_request_headers,
            }
          },
        ]
      : []

    const options: CheerioCrawlerOptions = {
      requestQueue,
      requestHandler: router as any,
      preNavigationHooks: preNavigationHooks,
      // Set a fixed concurrency to avoid autoscaling issues
      maxConcurrency: this.config.max_concurrency || 1,
      minConcurrency: 1,
      // Disable autoscaling to avoid storage issues
      autoscaledPoolOptions: {
        desiredConcurrency: 1,
        maxConcurrency: this.config.max_concurrency || 1,
      },
      ...(this.config.max_requests_per_minute && {
        maxRequestsPerMinute: this.config.max_requests_per_minute,
      }),
      ...(this.proxyConfiguration && {
        proxyConfiguration: this.proxyConfiguration,
      }),
    }

    console.log('CheerioCrawler options:', options)
    return options
  }

  createCrawlerInstance(options: CheerioCrawlerOptions): CrawleeCheerioCrawler {
    if (this.config.features?.pdf?.activated) {
      options.additionalMimeTypes = ['application/pdf']
    }

    // Disable session pool and autoscaling to avoid storage issues
    const crawlerOptions = {
      ...options,
      useSessionPool: false,
      persistCookiesPerSession: false,
      // Disable autoscaling completely
      autoscaledPoolOptions: {
        ...options.autoscaledPoolOptions,
        isTaskReadyFunction: async () => true,
        isFinishedFunction: async () => false,
      },
    }

    return new CrawleeCheerioCrawler(crawlerOptions)
  }

  override async defaultHandler(context: CheerioCrawlingContext) {
    await this.handlePage(context)
  }
}
