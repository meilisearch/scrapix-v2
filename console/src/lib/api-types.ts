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
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  config?: Record<string, any>;
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

// WebSocket envelope messages from /ws/job/{id}
// The server wraps events in WsServerMessage envelopes.
export type WsServerMessage =
  | { type: "event"; job_id: string; event: CrawlEvent }
  | { type: "status"; job_id: string; status: Job }
  | { type: "subscribed"; job_id: string }
  | { type: "unsubscribed"; job_id: string }
  | { type: "error"; message: string; code: string }
  | { type: "pong"; timestamp: number };

// Inner CrawlEvent — matches Rust CrawlEvent serde output
export type CrawlEvent =
  | { type: "job_started"; job_id: string; index_uid: string; start_urls: string[]; timestamp: number }
  | { type: "page_crawled"; job_id: string; url: string; status: number; content_length: number; duration_ms: number; timestamp: number }
  | { type: "page_failed"; job_id: string; url: string; error: string; retry_count: number; timestamp: number }
  | { type: "document_indexed"; job_id: string; url: string; document_id: string; timestamp: number }
  | { type: "urls_discovered"; job_id: string; source_url: string; count: number; timestamp: number }
  | { type: "job_completed"; job_id: string; pages_crawled: number; documents_indexed: number; errors: number; bytes_downloaded: number; duration_secs: number; timestamp: number }
  | { type: "job_failed"; job_id: string; error: string; timestamp: number }
  | { type: "page_skipped"; job_id: string; url: string; reason: string; timestamp: number }
  | { type: "rate_limited"; job_id: string; domain: string; wait_ms: number; timestamp: number };

// From GET /health/services
export interface ServiceHealth {
  services: ServiceStatus[];
}

export interface ServiceStatus {
  name: string;
  status: "up" | "idle" | "down";
  last_seen_secs_ago?: number;
}

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

// From GET /configs, POST /configs, etc.
export interface SavedConfig {
  id: string;
  account_id: string;
  name: string;
  description: string | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  config: Record<string, any>;
  cron_expression: string | null;
  cron_enabled: boolean;
  last_run_at: string | null;
  next_run_at: string | null;
  last_job_id: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateConfigRequest {
  name: string;
  description?: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  config: Record<string, any>;
  cron_expression?: string;
  cron_enabled?: boolean;
}

export interface UpdateConfigRequest {
  name?: string;
  description?: string | null;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  config?: Record<string, any>;
  cron_expression?: string | null;
  cron_enabled?: boolean;
}

export interface TriggerResponse {
  job_id: string;
  config_id: string;
  message: string;
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
