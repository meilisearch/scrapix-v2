"use client";

import { useState, useEffect, useCallback } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { submitScrape, createCrawl, fetchServiceHealth } from "@/lib/api";
import type { ScrapeResult, ServiceStatus } from "@/lib/api-types";
import { UrlBar } from "./url-bar";
import { ScrapeOptions, type ScrapeState } from "./scrape-options";
import { CrawlOptions, type CrawlState, defaultCrawlState } from "./crawl-options";
import { ResultPanel } from "./result-panel";
import { RecentRuns, loadRuns, saveRun, type RunEntry } from "./recent-runs";

export default function PlaygroundPage() {
  const [mode, setMode] = useState<"scrape" | "crawl">("scrape");
  const [url, setUrl] = useState("https://example.com");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<ScrapeResult | null>(null);
  const [crawlResult, setCrawlResult] = useState<{
    job_id: string;
    status: string;
    message?: string;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<RunEntry[]>([]);

  const [scrapeState, setScrapeState] = useState<ScrapeState>({
    formats: ["markdown", "metadata"],
    only_main_content: true,
    include_links: false,
    timeout_ms: "30000",
  });

  const [crawlState, setCrawlState] = useState<CrawlState>(defaultCrawlState);
  const [services, setServices] = useState<ServiceStatus[]>([]);

  useEffect(() => {
    setRuns(loadRuns());
  }, []);

  // Poll service health every 10s
  useEffect(() => {
    const poll = () =>
      fetchServiceHealth()
        .then((data) => setServices(data.services))
        .catch(() => {});
    poll();
    const interval = setInterval(poll, 10000);
    return () => clearInterval(interval);
  }, []);

  const handleScrape = useCallback(async () => {
    if (!url.trim()) {
      toast.error("Please enter a URL");
      return;
    }
    if (scrapeState.formats.length === 0) {
      toast.error("Select at least one output format");
      return;
    }

    setLoading(true);
    setResult(null);
    setCrawlResult(null);
    setError(null);

    try {
      const data = await submitScrape({
        url,
        formats: scrapeState.formats,
        only_main_content: scrapeState.only_main_content,
        include_links: scrapeState.include_links,
        timeout_ms: parseInt(scrapeState.timeout_ms) || 30000,
      });
      setResult(data);
      const newRuns = saveRun({
        id: Math.random().toString(36).slice(2) + Date.now().toString(36),
        type: "scrape",
        url,
        status_code: data.status_code,
        duration_ms: data.scrape_duration_ms,
        timestamp: new Date().toISOString(),
      });
      setRuns(newRuns);
    } catch (err) {
      const msg =
        err instanceof Error
          ? err.message
          : "Failed to fetch. Is the API running?";
      setError(msg);
    }

    setLoading(false);
  }, [url, scrapeState]);

  const handleCrawl = useCallback(async () => {
    if (!url.trim()) {
      toast.error("Please enter a URL");
      return;
    }

    const urls = url
      .split("\n")
      .map((u) => u.trim())
      .filter((u) => u);

    const indexUid =
      crawlState.index_uid.trim() || `playground-${Date.now()}`;

    setLoading(true);
    setResult(null);
    setCrawlResult(null);
    setError(null);

    // Helper to split newline-separated text into a filtered array
    const lines = (s: string) =>
      s.split("\n").map((l) => l.trim()).filter((l) => l);

    // Helper to parse an optional int
    const optInt = (s: string) => {
      const n = parseInt(s);
      return isNaN(n) ? undefined : n;
    };

    // Helper to parse an optional float
    const optFloat = (s: string) => {
      const n = parseFloat(s);
      return isNaN(n) ? undefined : n;
    };

    // Helper to parse optional JSON
    const optJson = (s: string) => {
      if (!s.trim()) return undefined;
      try {
        return JSON.parse(s);
      } catch {
        return undefined;
      }
    };

    // ── Build config ──
    const config: Record<string, unknown> = {
      start_urls: urls,
      index_uid: indexUid,
      crawler_type: crawlState.crawler_type,
      max_depth: optInt(crawlState.max_depth),
      max_pages: optInt(crawlState.max_pages),
    };

    // Meilisearch
    const ms: Record<string, unknown> = {
      url: crawlState.meilisearch_url,
      api_key: crawlState.meilisearch_api_key,
    };
    if (crawlState.meilisearch_primary_key.trim())
      ms.primary_key = crawlState.meilisearch_primary_key;
    const batchSize = optInt(crawlState.meilisearch_batch_size);
    if (batchSize && batchSize !== 1000) ms.batch_size = batchSize;
    if (crawlState.meilisearch_keep_settings) ms.keep_settings = true;
    config.meilisearch = ms;

    // Allowed domains
    const domains = lines(crawlState.allowed_domains);
    if (domains.length > 0) config.allowed_domains = domains;

    // URL patterns
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

    // Sitemap
    if (crawlState.sitemap_enabled) {
      const sitemapUrls = lines(crawlState.sitemap_urls);
      config.sitemap = {
        enabled: true,
        ...(sitemapUrls.length > 0 ? { urls: sitemapUrls } : {}),
      };
    }

    // Concurrency
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

    // Rate limiting
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

    // Features
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
      const ai: Record<string, unknown> = {
        enabled: true,
        prompt: crawlState.ai_extraction_prompt,
        model: crawlState.ai_extraction_model,
      };
      const maxTokens = optInt(crawlState.ai_extraction_max_tokens);
      if (maxTokens) ai.max_tokens = maxTokens;
      features.ai_extraction = ai;
    }
    if (crawlState.feat_ai_summary) features.ai_summary = { enabled: true };
    if (crawlState.feat_embeddings) {
      const emb: Record<string, unknown> = {
        enabled: true,
        model: crawlState.embeddings_model,
      };
      const dims = optInt(crawlState.embeddings_dimensions);
      if (dims) emb.dimensions = dims;
      features.embeddings = emb;
    }
    if (Object.keys(features).length > 0) config.features = features;

    // Headers
    const headers = optJson(crawlState.headers);
    if (headers) config.headers = headers;

    // User agents
    const uas = lines(crawlState.user_agents);
    if (uas.length > 0) config.user_agents = uas;

    // Proxy
    const proxyUrls = lines(crawlState.proxy_urls);
    if (proxyUrls.length > 0) {
      config.proxy = {
        urls: proxyUrls,
        rotation: crawlState.proxy_rotation,
      };
    }

    try {
      const data = await createCrawl(config);
      setCrawlResult({
        job_id: data.job_id,
        status: "created",
        message: `Crawl job submitted for ${urls.length} URL(s)`,
      });
      const newRuns = saveRun({
        id: Math.random().toString(36).slice(2) + Date.now().toString(36),
        type: "crawl",
        url: urls[0],
        timestamp: new Date().toISOString(),
      });
      setRuns(newRuns);
    } catch (err) {
      const msg =
        err instanceof Error
          ? err.message
          : "Failed to fetch. Is the API running?";
      setError(msg);
    }

    setLoading(false);
  }, [url, crawlState]);

  const handleSubmit = mode === "scrape" ? handleScrape : handleCrawl;

  const handleReplay = (run: RunEntry) => {
    setMode(run.type);
    setUrl(run.url);
  };

  return (
    <div className="flex flex-col gap-4 h-[calc(100vh-6rem)]">
      {/* Service health bar */}
      {services.length > 0 && (
        <div className="flex items-center gap-3 px-1">
          {services.map((svc) => (
            <Badge
              key={svc.name}
              variant="outline"
              className="text-xs gap-1.5 font-normal"
            >
              <span
                className={`inline-block h-1.5 w-1.5 rounded-full ${
                  svc.status === "up"
                    ? "bg-green-500"
                    : svc.status === "idle"
                      ? "bg-yellow-500"
                      : "bg-gray-400"
                }`}
              />
              {svc.name}
            </Badge>
          ))}
        </div>
      )}

      {/* URL Bar */}
      <UrlBar
        mode={mode}
        onModeChange={setMode}
        url={url}
        onUrlChange={setUrl}
        onSubmit={handleSubmit}
        loading={loading}
      />

      {/* Main panels */}
      <div className="grid grid-cols-1 lg:grid-cols-[2fr_3fr] gap-4 flex-1 min-h-0">
        <Card className="overflow-auto">
          <CardContent className="p-4">
            {mode === "scrape" ? (
              <ScrapeOptions state={scrapeState} onChange={setScrapeState} />
            ) : (
              <CrawlOptions state={crawlState} onChange={setCrawlState} />
            )}
          </CardContent>
        </Card>

        <Card className="overflow-hidden">
          <CardContent className="p-4 h-full">
            <ResultPanel
              result={result}
              crawlResult={crawlResult}
              mode={mode}
              loading={loading}
              error={error}
            />
          </CardContent>
        </Card>
      </div>

      {/* Recent runs */}
      <RecentRuns runs={runs} onReplay={handleReplay} />
    </div>
  );
}
