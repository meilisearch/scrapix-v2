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
const WS_BASE =
  (process.env.NEXT_PUBLIC_SCRAPIX_API_URL || "http://localhost:8080").replace(
    /^http/,
    "ws"
  );

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
  return `${WS_BASE}${path}`;
}
