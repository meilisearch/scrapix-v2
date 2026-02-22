import type {
  SystemStats,
  Job,
  JobStatus,
  CrawlConfig,
  RecentErrors,
  ScrapeResult,
  ServiceHealth,
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

/** WebSocket URL pointing directly at the backend (rewrites don't proxy WS). */
export function wsUrl(path: string): string {
  return `${getWsBase()}${path}`;
}
