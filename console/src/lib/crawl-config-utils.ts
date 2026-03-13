import type { CrawlState } from "@/app/(dashboard)/playground/crawl-options";
import { defaultCrawlState } from "@/app/(dashboard)/playground/crawl-options";

/**
 * Convert CrawlState form data to a crawl config JSON object.
 * Extracted from the crawl page's handleCrawl logic.
 */
export function crawlStateToConfig(
  crawlState: CrawlState,
  startUrls: string[]
): Record<string, unknown> {
  const lines = (s: string) =>
    s
      .split("\n")
      .map((l) => l.trim())
      .filter((l) => l);

  const optInt = (s: string) => {
    const n = parseInt(s);
    return isNaN(n) ? undefined : n;
  };

  const optFloat = (s: string) => {
    const n = parseFloat(s);
    return isNaN(n) ? undefined : n;
  };

  const optJson = (s: string) => {
    if (!s.trim()) return undefined;
    try {
      return JSON.parse(s);
    } catch {
      return undefined;
    }
  };

  const indexUid =
    crawlState.index_uid.trim() ||
    startUrls[0]
      ?.replace(/^https?:\/\//, "")
      .replace(/\/+$/, "")
      .replace(/[.\/:]/g, "-")
      .replace(/-+/g, "-") ||
    `crawl-${Date.now()}`;

  const maxDepth = optInt(crawlState.max_depth);
  const maxPages = optInt(crawlState.max_pages);

  const config: Record<string, unknown> = {
    start_urls: startUrls,
    index_uid: indexUid,
    ...(crawlState.source.trim() ? { source: crawlState.source.trim() } : {}),
    crawler_type: crawlState.crawler_type,
    ...(maxDepth && maxDepth > 0 ? { max_depth: maxDepth } : {}),
    ...(maxPages ? { max_pages: maxPages } : {}),
  };

  config.index_strategy = crawlState.index_strategy;

  const ms: Record<string, unknown> = {
    url: crawlState.meilisearch_url,
    api_key: crawlState.meilisearch_api_key,
  };
  if (crawlState.meilisearch_primary_key.trim())
    ms.primary_key = crawlState.meilisearch_primary_key;
  const batchSize = optInt(crawlState.meilisearch_batch_size);
  if (batchSize && batchSize !== 1000) ms.batch_size = batchSize;
  ms.keep_settings = crawlState.meilisearch_keep_settings;
  config.meilisearch = ms;

  const domains = lines(crawlState.allowed_domains);
  if (domains.length > 0) config.allowed_domains = domains;

  const incl = lines(crawlState.include_patterns);
  const excl = lines(crawlState.exclude_patterns);
  const indexOnly = lines(crawlState.index_only_patterns);
  if (incl.length > 0 || excl.length > 0 || indexOnly.length > 0) {
    const patterns: Record<string, string[]> = {};
    if (incl.length > 0) patterns.include = incl;
    if (excl.length > 0) patterns.exclude = excl;
    if (indexOnly.length > 0) patterns.index_only = indexOnly;
    config.url_patterns = patterns;
  }

  const sitemapUrls = lines(crawlState.sitemap_urls);
  config.sitemap = {
    enabled: crawlState.sitemap_enabled,
    ...(sitemapUrls.length > 0 ? { urls: sitemapUrls } : {}),
  };

  const maxConcurrent = optInt(crawlState.max_concurrent_requests);
  const browserPool = optInt(crawlState.browser_pool_size);
  const dnsConcurrency = optInt(crawlState.dns_concurrency);
  if (
    (maxConcurrent && maxConcurrent !== 50) ||
    (browserPool && browserPool !== 5) ||
    (dnsConcurrency && dnsConcurrency !== 100)
  ) {
    config.concurrency = {
      ...(maxConcurrent ? { max_concurrent_requests: maxConcurrent } : {}),
      ...(browserPool ? { browser_pool_size: browserPool } : {}),
      ...(dnsConcurrency ? { dns_concurrency: dnsConcurrency } : {}),
    };
  }

  const rateLimit: Record<string, unknown> = {
    respect_robots_txt: crawlState.respect_robots,
  };
  const rps = optFloat(crawlState.requests_per_second);
  const rpm = optInt(crawlState.requests_per_minute);
  const domainDelay = optInt(crawlState.per_domain_delay_ms);
  const crawlDelay = optInt(crawlState.default_crawl_delay_ms);
  if (rps) rateLimit.requests_per_second = rps;
  if (rpm) rateLimit.requests_per_minute = rpm;
  if (domainDelay && domainDelay !== 100)
    rateLimit.per_domain_delay_ms = domainDelay;
  if (crawlDelay && crawlDelay !== 1000)
    rateLimit.default_crawl_delay_ms = crawlDelay;
  config.rate_limit = rateLimit;

  const features: Record<string, unknown> = {};
  if (!crawlState.feat_metadata) features.metadata = { enabled: false };
  if (!crawlState.feat_markdown) features.markdown = { enabled: false };
  if (crawlState.feat_block_split) features.block_split = { enabled: true };
  if (crawlState.feat_schema) {
    const schema: Record<string, unknown> = { enabled: true };
    const types = crawlState.schema_only_types
      .split(",")
      .map((t) => t.trim())
      .filter((t) => t);
    if (types.length > 0) schema.only_types = types;
    schema.convert_dates = crawlState.schema_convert_dates;
    features.schema = schema;
  }
  if (crawlState.feat_custom_selectors) {
    const selectors = optJson(crawlState.custom_selectors);
    if (selectors) {
      features.custom_selectors = { enabled: true, selectors };
    }
  }
  if (crawlState.feat_ai_extraction) {
    features.ai_extraction = {
      enabled: true,
      prompt: crawlState.ai_extraction_prompt,
    };
  }
  if (crawlState.feat_ai_summary) features.ai_summary = { enabled: true };
  if (Object.keys(features).length > 0) config.features = features;

  const headers = optJson(crawlState.headers);
  if (headers) config.headers = headers;

  const uas = lines(crawlState.user_agents);
  if (uas.length > 0) config.user_agents = uas;

  const proxyUrls = lines(crawlState.proxy_urls);
  if (proxyUrls.length > 0) {
    config.proxy = {
      urls: proxyUrls,
      rotation: crawlState.proxy_rotation,
    };
  }

  return config;
}

type AnyConfig = Record<string, unknown>;

function str(val: unknown): string {
  if (val === undefined || val === null) return "";
  return String(val);
}

function arrToLines(val: unknown): string {
  if (!Array.isArray(val)) return "";
  return val.join("\n");
}

/**
 * Convert a crawl config JSON object back to CrawlState form data.
 * Used when editing an existing config.
 */
export function configToCrawlState(config: AnyConfig): CrawlState {
  const state: CrawlState = { ...defaultCrawlState };

  state.index_uid = str(config.index_uid);
  state.source = str(config.source);
  state.max_depth = str(config.max_depth);
  state.max_pages = str(config.max_pages) || defaultCrawlState.max_pages;
  state.crawler_type =
    config.crawler_type === "browser" ? "browser" : "http";

  // Sitemap
  const sitemap = config.sitemap as AnyConfig | undefined;
  if (sitemap) {
    state.sitemap_enabled = sitemap.enabled !== false;
    state.sitemap_urls = arrToLines(sitemap.urls);
  }

  // Rate limit
  const rateLimit = config.rate_limit as AnyConfig | undefined;
  if (rateLimit) {
    state.respect_robots =
      rateLimit.respect_robots_txt !== false;
    state.requests_per_second = str(rateLimit.requests_per_second);
    state.requests_per_minute = str(rateLimit.requests_per_minute);
    state.per_domain_delay_ms =
      str(rateLimit.per_domain_delay_ms) ||
      defaultCrawlState.per_domain_delay_ms;
    state.default_crawl_delay_ms =
      str(rateLimit.default_crawl_delay_ms) ||
      defaultCrawlState.default_crawl_delay_ms;
  }

  // Domains & patterns
  state.allowed_domains = arrToLines(config.allowed_domains);
  const urlPatterns = config.url_patterns as AnyConfig | undefined;
  if (urlPatterns) {
    state.include_patterns = arrToLines(urlPatterns.include);
    state.exclude_patterns = arrToLines(urlPatterns.exclude);
    state.index_only_patterns = arrToLines(urlPatterns.index_only);
  }

  // Concurrency
  const concurrency = config.concurrency as AnyConfig | undefined;
  if (concurrency) {
    state.max_concurrent_requests =
      str(concurrency.max_concurrent_requests) ||
      defaultCrawlState.max_concurrent_requests;
    state.browser_pool_size =
      str(concurrency.browser_pool_size) ||
      defaultCrawlState.browser_pool_size;
    state.dns_concurrency =
      str(concurrency.dns_concurrency) || defaultCrawlState.dns_concurrency;
  }

  // Features
  const features = config.features as AnyConfig | undefined;
  if (features) {
    const metadata = features.metadata as AnyConfig | undefined;
    state.feat_metadata = metadata ? metadata.enabled !== false : true;

    const markdown = features.markdown as AnyConfig | undefined;
    state.feat_markdown = markdown ? markdown.enabled !== false : true;

    const blockSplit = features.block_split as AnyConfig | undefined;
    state.feat_block_split = blockSplit?.enabled === true;

    const schema = features.schema as AnyConfig | undefined;
    if (schema) {
      state.feat_schema = schema.enabled === true;
      const types = schema.only_types as string[] | undefined;
      state.schema_only_types = types ? types.join(", ") : "";
      state.schema_convert_dates = schema.convert_dates !== false;
    }

    const customSelectors = features.custom_selectors as AnyConfig | undefined;
    if (customSelectors) {
      state.feat_custom_selectors = customSelectors.enabled === true;
      state.custom_selectors = customSelectors.selectors
        ? JSON.stringify(customSelectors.selectors)
        : "";
    }

    const aiExtraction = features.ai_extraction as AnyConfig | undefined;
    if (aiExtraction) {
      state.feat_ai_extraction = aiExtraction.enabled === true;
      state.ai_extraction_prompt = str(aiExtraction.prompt);
    }

    const aiSummary = features.ai_summary as AnyConfig | undefined;
    state.feat_ai_summary = aiSummary?.enabled === true;
  }

  // Headers
  if (config.headers && typeof config.headers === "object") {
    state.headers = JSON.stringify(config.headers);
  }

  // User agents
  state.user_agents = arrToLines(config.user_agents);

  // Proxy
  const proxy = config.proxy as AnyConfig | undefined;
  if (proxy) {
    state.proxy_urls = arrToLines(proxy.urls);
    const rotation = str(proxy.rotation);
    if (
      rotation === "round_robin" ||
      rotation === "random" ||
      rotation === "least_used"
    ) {
      state.proxy_rotation = rotation;
    }
  }

  // Index strategy (also supports legacy replace_index boolean)
  const strategy = str(config.index_strategy);
  if (strategy === "update" || strategy === "replace") {
    state.index_strategy = strategy;
  } else if (config.replace_index === true) {
    state.index_strategy = "replace";
  }

  // Meilisearch
  const ms = config.meilisearch as AnyConfig | undefined;
  if (ms) {
    state.meilisearch_url = str(ms.url) || defaultCrawlState.meilisearch_url;
    state.meilisearch_api_key =
      str(ms.api_key) || defaultCrawlState.meilisearch_api_key;
    state.meilisearch_primary_key = str(ms.primary_key);
    state.meilisearch_batch_size =
      str(ms.batch_size) || defaultCrawlState.meilisearch_batch_size;
    state.meilisearch_keep_settings = ms.keep_settings === true;
  }

  return state;
}

/**
 * Extract start_urls from a config object.
 */
export function getStartUrls(config: AnyConfig): string[] {
  const urls = config.start_urls;
  if (Array.isArray(urls)) return urls.map(String);
  return [];
}
