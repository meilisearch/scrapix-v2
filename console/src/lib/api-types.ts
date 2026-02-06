// From GET /stats
export interface SystemStats {
  total_jobs: number;
  active_jobs: number;
  completed_jobs: number;
  failed_jobs: number;
  total_pages_crawled: number;
  total_pages_failed: number;
  domains_tracked: number;
  success_rate: number;
}

// From GET /jobs
export interface Job {
  id: string;
  status: "pending" | "running" | "completed" | "failed" | "cancelled";
  config: CrawlConfig;
  created_at: string;
  started_at?: string;
  completed_at?: string;
  pages_crawled: number;
  pages_failed: number;
  pages_total?: number;
}

export interface CrawlConfig {
  start_urls: string[];
  max_depth?: number;
  max_pages?: number;
  allowed_domains?: string[];
  index_uid: string;
}

// From GET /job/{id}/status
export interface JobStatus {
  id: string;
  status: string;
  pages_crawled: number;
  pages_failed: number;
  pages_total?: number;
  current_depth?: number;
  urls_in_queue?: number;
  started_at?: string;
  elapsed_seconds?: number;
}

// WebSocket messages from /ws/job/{id}
export type WsMessage =
  | { type: "page_crawled"; url: string; status: number; elapsed_ms: number }
  | { type: "page_failed"; url: string; error: string }
  | {
      type: "job_progress";
      pages_crawled: number;
      pages_failed: number;
      pages_total?: number;
    }
  | { type: "job_completed"; pages_crawled: number; pages_failed: number }
  | { type: "job_failed"; error: string };

// From GET /errors
export interface RecentErrors {
  errors: ErrorEntry[];
  total: number;
  by_status: Record<string, number>;
  by_domain: Record<string, number>;
}

export interface ErrorEntry {
  url: string;
  status?: number;
  error: string;
  domain: string;
  timestamp: string;
}

// From POST /scrape
export interface ScrapeResult {
  url: string;
  title?: string;
  content?: string;
  markdown?: string;
  html?: string;
  metadata?: Record<string, string>;
}
