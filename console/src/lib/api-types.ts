// From GET /stats
export interface SystemStats {
  meilisearch: {
    available: boolean;
    url: string;
  };
  jobs: {
    total: number;
    running: number;
    completed: number;
    failed: number;
    pending: number;
  };
  diagnostics: {
    recent_errors_count: number;
    tracked_domains: number;
    total_requests: number;
    total_successes: number;
    total_failures: number;
  };
  collected_at: string;
}

// From GET /jobs — array of JobStatusResponse
// Also used by GET /job/{id}/status
export interface Job {
  job_id: string;
  status: string;
  index_uid: string;
  pages_crawled: number;
  pages_indexed: number;
  documents_sent: number;
  errors: number;
  started_at?: string;
  completed_at?: string;
  duration_seconds?: number;
  error_message?: string;
  crawl_rate: number;
  eta_seconds?: number;
  start_urls?: string[];
  max_pages?: number;
}

// Alias for backwards compat
export type JobStatus = Job;

export interface CrawlConfig {
  start_urls: string[];
  max_depth?: number;
  max_pages?: number;
  allowed_domains?: string[];
  index_uid: string;
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
  total_count: number;
  by_status: Record<string, number>;
  by_domain: Array<{ domain: string; count: number }>;
  source: string;
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
  success: boolean;
  url: string;
  status_code: number;
  scrape_duration_ms: number;
  markdown?: string;
  html?: string;
  raw_html?: string;
  content?: string;
  links?: string[];
  language?: string;
  metadata?: ScrapeMetadata;
}

export interface ScrapeMetadata {
  title?: string;
  description?: string;
  author?: string;
  keywords: string[];
  canonical_url?: string;
  published_date?: string;
  open_graph: Record<string, string>;
  twitter: Record<string, string>;
}
