import type {
  SystemStats,
  Job,
  JobStatus,
  CrawlConfig,
  RecentErrors,
  ScrapeResult,
  ServiceHealth,
  SavedConfig,
  CreateConfigRequest,
  UpdateConfigRequest,
  TriggerResponse,
  MeilisearchEngine,
  CreateEngineRequest,
  UpdateEngineRequest,
  MeilisearchIndex,
  MapResult,
  AnalyticsResponse,
  HourlyStatsRow,
  KpisRow,
  AccountUsageRow,
  DailyUsageRow,
  TopDomainRow,
} from "./api-types";

// API calls go through Next.js rewrites (/api/scrapix/* → backend) to avoid CORS.
// WebSocket still connects directly to the backend.
const BASE = "/api/scrapix";

/** Derive the WebSocket base URL at runtime, so it works on any deployment. */
function getWsBase(): string {
  // 1. Explicit env var (baked at build time for NEXT_PUBLIC_*)
  const env = process.env.NEXT_PUBLIC_SCRAPIX_API_URL;
  if (env) return env.replace(/^http/, "ws");

  // 2. Browser: derive from current page URL
  if (typeof window !== "undefined") {
    const { protocol, hostname } = window.location;
    const wsProtocol = protocol === "https:" ? "wss:" : "ws:";

    // console.X → api.X (e.g. console.scrapix.meilisearch.net → api.scrapix.meilisearch.net)
    if (hostname.startsWith("console.")) {
      return `${wsProtocol}//${hostname.replace(/^console\./, "api.")}`;
    }

    // Local dev: API on port 8080
    return `${wsProtocol}//${hostname}:8080`;
  }

  // 3. Server-side fallback
  return "ws://localhost:8080";
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    credentials: "include",
    ...init,
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(
      body || `API error: ${res.status} ${res.statusText}`
    );
  }
  return res.json();
}

export async function fetchStats(): Promise<SystemStats> {
  return request("/stats");
}

export async function fetchJobs(): Promise<Job[]> {
  return request("/jobs");
}

export async function fetchJobStatus(id: string): Promise<JobStatus> {
  return request(`/job/${encodeURIComponent(id)}/status`);
}

export async function deleteJob(id: string): Promise<void> {
  await fetch(`${BASE}/job/${encodeURIComponent(id)}`, {
    method: "DELETE",
  });
}

export async function fetchServiceHealth(): Promise<ServiceHealth> {
  return request("/health/services");
}

export async function fetchErrors(last?: number): Promise<RecentErrors> {
  const params = last != null ? `?last=${last}` : "";
  return request(`/errors${params}`);
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export async function createCrawl(config: Record<string, any>): Promise<{ job_id: string }> {
  return request("/crawl", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(config),
  });
}

export interface ScrapeOptions {
  url: string;
  formats: string[];
  only_main_content?: boolean;
  include_links?: boolean;
  timeout_ms?: number;
  headers?: Record<string, string>;
}

export async function submitScrape(opts: ScrapeOptions): Promise<ScrapeResult> {
  return request("/scrape", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(opts),
  });
}

// ============================================================================
// Map
// ============================================================================

export interface MapOptions {
  url: string;
  limit?: number;
  depth?: number;
  search?: string;
  sitemap?: boolean;
}

export async function submitMap(opts: MapOptions): Promise<MapResult> {
  return request("/map", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(opts),
  });
}

// ============================================================================
// Saved Configs
// ============================================================================

export async function fetchConfigs(): Promise<SavedConfig[]> {
  return request("/configs");
}

export async function fetchConfig(id: string): Promise<SavedConfig> {
  return request(`/configs/${encodeURIComponent(id)}`);
}

export async function createConfig(req: CreateConfigRequest): Promise<SavedConfig> {
  return request("/configs", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
}

export async function updateConfig(id: string, req: UpdateConfigRequest): Promise<SavedConfig> {
  return request(`/configs/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
}

export async function deleteConfig(id: string): Promise<void> {
  await fetch(`${BASE}/configs/${encodeURIComponent(id)}`, {
    method: "DELETE",
    credentials: "include",
  });
}

export async function triggerConfig(id: string): Promise<TriggerResponse> {
  return request(`/configs/${encodeURIComponent(id)}/trigger`, {
    method: "POST",
  });
}

// ============================================================================
// Meilisearch Engines
// ============================================================================

export async function fetchEngines(): Promise<MeilisearchEngine[]> {
  return request("/engines");
}

export async function fetchEngine(id: string): Promise<MeilisearchEngine> {
  return request(`/engines/${encodeURIComponent(id)}`);
}

export async function createEngine(req: CreateEngineRequest): Promise<MeilisearchEngine> {
  return request("/engines", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
}

export async function updateEngine(id: string, req: UpdateEngineRequest): Promise<MeilisearchEngine> {
  return request(`/engines/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(req),
  });
}

export async function deleteEngine(id: string): Promise<void> {
  await fetch(`${BASE}/engines/${encodeURIComponent(id)}`, {
    method: "DELETE",
    credentials: "include",
  });
}

export async function setDefaultEngine(id: string): Promise<MeilisearchEngine> {
  return request(`/engines/${encodeURIComponent(id)}/default`, {
    method: "POST",
  });
}

export async function fetchEngineIndexes(id: string): Promise<MeilisearchIndex[]> {
  return request(`/engines/${encodeURIComponent(id)}/indexes`);
}

/** WebSocket URL pointing directly at the backend (rewrites don't proxy WS). */
export function wsUrl(path: string): string {
  return `${getWsBase()}${path}`;
}

// ============================================================================
// Analytics
// ============================================================================

export async function fetchKpis(hours: number = 24): Promise<AnalyticsResponse<KpisRow>> {
  return request(`/analytics/v0/pipes/kpis.json?hours=${hours}`);
}

export async function fetchHourlyStats(hours: number = 24): Promise<AnalyticsResponse<HourlyStatsRow>> {
  return request(`/analytics/v0/pipes/hourly_stats.json?hours=${hours}`);
}

export async function fetchAccountUsage(accountId: string, hours: number = 24): Promise<AnalyticsResponse<AccountUsageRow>> {
  return request(`/analytics/v0/pipes/account_usage.json?account_id=${encodeURIComponent(accountId)}&hours=${hours}`);
}

export async function fetchDailyUsage(accountId: string, days: number = 30): Promise<AnalyticsResponse<DailyUsageRow>> {
  return request(`/analytics/v0/pipes/account_daily_usage.json?account_id=${encodeURIComponent(accountId)}&days=${days}`);
}

export async function fetchTopDomains(hours: number = 24, limit: number = 10): Promise<AnalyticsResponse<TopDomainRow>> {
  return request(`/analytics/v0/pipes/top_domains.json?hours=${hours}&limit=${limit}`);
}
