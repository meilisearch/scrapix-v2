import type {
  SystemStats,
  Job,
  JobStatus,
  CrawlConfig,
  RecentErrors,
  ScrapeResult,
} from "./api-types";

const BASE =
  process.env.NEXT_PUBLIC_SCRAPIX_API_URL || "http://localhost:8080";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, init);
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
  await fetch(`${BASE}/job/${encodeURIComponent(id)}`, { method: "DELETE" });
}

export async function fetchErrors(last?: number): Promise<RecentErrors> {
  const params = last != null ? `?last=${last}` : "";
  return request(`/errors${params}`);
}

export async function createCrawl(
  config: CrawlConfig
): Promise<{ job_id: string }> {
  return request("/crawl", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(config),
  });
}

export async function submitScrape(
  url: string,
  format: string
): Promise<ScrapeResult> {
  return request("/scrape", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ url, format }),
  });
}

/** Derive the WebSocket URL from the configured API base URL. */
export function wsUrl(path: string): string {
  const base = BASE.replace(/^http/, "ws");
  return `${base}${path}`;
}
