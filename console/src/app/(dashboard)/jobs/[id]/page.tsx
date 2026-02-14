"use client";

import { useEffect, useState, useRef, useCallback } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import {
  ArrowLeft,
  Trash2,
  AlertCircle,
  Globe,
  Clock,
  FileText,
  Zap,
  ExternalLink,
  Settings2,
  ChevronDown,
} from "lucide-react";
import { fetchJobStatus, deleteJob, wsUrl } from "@/lib/api";
import type { JobStatus, WsServerMessage, CrawlEvent } from "@/lib/api-types";
import { toast } from "sonner";
import { formatDistanceToNow } from "date-fns";

interface LogEntry {
  time: string;
  message: string;
  variant: "default" | "error";
}

const statusVariant: Record<
  string,
  "default" | "secondary" | "destructive" | "outline"
> = {
  completed: "default",
  running: "secondary",
  pending: "outline",
  failed: "destructive",
  cancelled: "destructive",
  paused: "outline",
};

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ConfigSummary({ config }: { config: Record<string, any> }) {
  // Group config entries into sections
  const mainFields: [string, string][] = [];
  const meilisearch: [string, string][] = [];
  const patterns: [string, string][] = [];
  const other: [string, string][] = [];

  // start_urls already shown in the "Crawling" card, skip it
  // index_uid already shown in "Index" card, skip it
  const skip = new Set(["start_urls", "index_uid"]);

  const formatValue = (v: unknown): string => {
    if (v == null) return "—";
    if (typeof v === "boolean") return v ? "Yes" : "No";
    if (Array.isArray(v)) return v.length === 0 ? "—" : v.join(", ");
    if (typeof v === "object") return JSON.stringify(v);
    return String(v);
  };

  const labelMap: Record<string, string> = {
    max_depth: "Max Depth",
    max_pages: "Max Pages",
    crawler_type: "Crawler Type",
    allowed_domains: "Allowed Domains",
  };

  for (const [key, value] of Object.entries(config)) {
    if (skip.has(key)) continue;
    // Skip empty/default nested objects
    if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      const entries = Object.entries(value).filter(
        ([, v]) =>
          v != null &&
          v !== "" &&
          v !== false &&
          !(Array.isArray(v) && v.length === 0)
      );
      if (entries.length === 0) continue;

      if (key === "meilisearch") {
        for (const [k, v] of entries) {
          // Mask API key
          const display =
            k === "api_key" && typeof v === "string" && v.length > 4
              ? `${v.slice(0, 4)}${"*".repeat(Math.min(v.length - 4, 20))}`
              : formatValue(v);
          meilisearch.push([k.replace(/_/g, " "), display]);
        }
      } else if (key === "url_patterns") {
        for (const [k, v] of entries) {
          patterns.push([k.replace(/_/g, " "), formatValue(v)]);
        }
      } else {
        for (const [k, v] of entries) {
          other.push([`${key}.${k}`, formatValue(v)]);
        }
      }
      continue;
    }

    if (key in labelMap || ["max_depth", "max_pages", "crawler_type", "allowed_domains"].includes(key)) {
      const display = formatValue(value);
      if (display !== "—") mainFields.push([labelMap[key] || key, display]);
    } else {
      const display = formatValue(value);
      if (display !== "—") other.push([key.replace(/_/g, " "), display]);
    }
  }

  const Section = ({
    title,
    entries,
  }: {
    title: string;
    entries: [string, string][];
  }) =>
    entries.length > 0 ? (
      <div>
        <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">
          {title}
        </p>
        <div className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1">
          {entries.map(([label, value]) => (
            <div key={label} className="contents">
              <span className="text-sm text-muted-foreground capitalize">
                {label}
              </span>
              <span className="text-sm font-mono truncate">{value}</span>
            </div>
          ))}
        </div>
      </div>
    ) : null;

  return (
    <div className="space-y-4">
      <Section title="Crawl" entries={mainFields} />
      {patterns.length > 0 && (
        <Section title="URL Patterns" entries={patterns} />
      )}
      {meilisearch.length > 0 && (
        <Section title="Meilisearch" entries={meilisearch} />
      )}
      {other.length > 0 && <Section title="Other" entries={other} />}
    </div>
  );
}

export default function JobDetailPage() {
  const { id } = useParams<{ id: string }>();
  const router = useRouter();
  const [status, setStatus] = useState<JobStatus | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [apiError, setApiError] = useState<string | null>(null);
  const [wsConnected, setWsConnected] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);

  const addLog = useCallback(
    (message: string, variant: "default" | "error" = "default") => {
      setLogs((prev) => {
        const next = [
          ...prev,
          { time: new Date().toLocaleTimeString(), message, variant },
        ];
        return next.length > 500 ? next.slice(-500) : next;
      });
    },
    []
  );

  // Poll status periodically (not just once)
  useEffect(() => {
    if (!id) return;
    const poll = () =>
      fetchJobStatus(id)
        .then((data) => {
          setStatus(data);
          setApiError(null);
        })
        .catch((e) =>
          setApiError(e instanceof Error ? e.message : "Failed to fetch job")
        );
    poll();
    const interval = setInterval(poll, 3000);
    return () => clearInterval(interval);
  }, [id]);

  // WebSocket connection
  useEffect(() => {
    if (!id) return;

    let reconnectTimer: ReturnType<typeof setTimeout>;

    function connect() {
      const ws = new WebSocket(wsUrl(`/ws/job/${id}`));
      wsRef.current = ws;

      ws.onopen = () => {
        setWsConnected(true);
        addLog("Connected to live feed");
      };

      ws.onmessage = (rawEvent) => {
        try {
          const msg: WsServerMessage = JSON.parse(rawEvent.data);

          if (msg.type === "status") {
            // Initial status snapshot on connect or on request
            setStatus(msg.status);
            addLog(
              `Status: ${msg.status.status} — ${msg.status.pages_crawled} crawled, ${msg.status.pages_indexed} indexed, ${msg.status.errors} errors`
            );
            return;
          }

          if (msg.type === "event") {
            const evt: CrawlEvent = msg.event;
            switch (evt.type) {
              case "page_crawled":
                addLog(
                  `Crawled ${evt.url} (${evt.status}) in ${evt.duration_ms}ms`
                );
                setStatus((prev) =>
                  prev
                    ? { ...prev, pages_crawled: prev.pages_crawled + 1 }
                    : prev
                );
                break;
              case "page_failed":
                addLog(`Failed ${evt.url}: ${evt.error}`, "error");
                setStatus((prev) =>
                  prev ? { ...prev, errors: prev.errors + 1 } : prev
                );
                break;
              case "document_indexed":
                addLog(`Indexed ${evt.url} → ${evt.document_id}`);
                setStatus((prev) =>
                  prev
                    ? {
                        ...prev,
                        pages_indexed: prev.pages_indexed + 1,
                        documents_sent: prev.documents_sent + 1,
                      }
                    : prev
                );
                break;
              case "urls_discovered":
                addLog(
                  `Discovered ${evt.count} URLs from ${evt.source_url}`
                );
                break;
              case "page_skipped":
                addLog(`Skipped ${evt.url}: ${evt.reason}`);
                break;
              case "rate_limited":
                addLog(
                  `Rate limited on ${evt.domain}, waiting ${evt.wait_ms}ms`
                );
                break;
              case "job_started":
                addLog(
                  `Job started: crawling ${evt.start_urls.length} seed URL(s)`
                );
                setStatus((prev) =>
                  prev ? { ...prev, status: "running" } : prev
                );
                break;
              case "job_completed":
                addLog(
                  `Job completed: ${evt.pages_crawled} crawled, ${evt.documents_indexed} indexed, ${evt.errors} errors in ${evt.duration_secs}s`
                );
                setStatus((prev) =>
                  prev ? { ...prev, status: "completed" } : prev
                );
                break;
              case "job_failed":
                addLog(`Job failed: ${evt.error}`, "error");
                setStatus((prev) =>
                  prev ? { ...prev, status: "failed" } : prev
                );
                break;
            }
            return;
          }

          // Ignore subscribed/unsubscribed/pong/error envelopes
        } catch {
          // ignore parse errors
        }
      };

      ws.onclose = () => {
        setWsConnected(false);
        reconnectTimer = setTimeout(connect, 3000);
      };

      ws.onerror = () => {
        ws.close();
      };
    }

    connect();

    return () => {
      clearTimeout(reconnectTimer);
      wsRef.current?.close();
    };
  }, [id, addLog]);

  // Auto-scroll logs
  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight });
  }, [logs]);

  const handleDelete = async () => {
    if (!id) return;
    if (!confirm("Are you sure you want to delete this job?")) return;
    try {
      await deleteJob(id);
      toast.success("Job deleted");
      router.push("/jobs");
    } catch {
      toast.error("Failed to delete job");
    }
  };

  const formatDuration = (seconds: number) => {
    if (seconds < 60) return `${seconds}s`;
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return `${m}m ${s}s`;
  };

  const progressPercent =
    status?.max_pages && status.max_pages > 0
      ? Math.min(
          100,
          Math.round((status.pages_crawled / status.max_pages) * 100)
        )
      : null;

  const isRunning = status?.status === "running";

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" asChild>
            <Link href="/jobs">
              <ArrowLeft className="h-4 w-4" />
            </Link>
          </Button>
          <div>
            <div className="flex items-center gap-3">
              <h2 className="text-2xl font-bold tracking-tight">Job Detail</h2>
              {status && (
                <Badge
                  variant={statusVariant[status.status] || "outline"}
                  className="text-sm"
                >
                  {isRunning && (
                    <span className="relative mr-1.5 flex h-2 w-2">
                      <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-current opacity-75" />
                      <span className="relative inline-flex h-2 w-2 rounded-full bg-current" />
                    </span>
                  )}
                  {status.status}
                </Badge>
              )}
            </div>
            <p className="text-muted-foreground font-mono text-xs">{id}</p>
          </div>
        </div>
        <Button variant="destructive" size="sm" onClick={handleDelete}>
          <Trash2 className="mr-2 h-4 w-4" />
          Delete
        </Button>
      </div>

      {apiError && (
        <Card className="border-destructive">
          <CardContent className="py-4 flex items-center gap-3">
            <AlertCircle className="h-5 w-5 text-destructive" />
            <p className="text-sm text-destructive">{apiError}</p>
          </CardContent>
        </Card>
      )}

      {status && (
        <>
          {/* Start URLs - the most important context */}
          {status.start_urls && status.start_urls.length > 0 && (
            <Card>
              <CardContent className="py-4">
                <div className="flex items-start gap-3">
                  <Globe className="h-5 w-5 text-muted-foreground mt-0.5 shrink-0" />
                  <div className="space-y-1 min-w-0">
                    <p className="text-sm font-medium text-muted-foreground">
                      Crawling
                    </p>
                    {status.start_urls.map((url) => (
                      <div key={url} className="flex items-center gap-2">
                        <p className="font-mono text-sm truncate">{url}</p>
                        <a
                          href={url}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="text-muted-foreground hover:text-foreground shrink-0"
                        >
                          <ExternalLink className="h-3 w-3" />
                        </a>
                      </div>
                    ))}
                  </div>
                </div>
              </CardContent>
            </Card>
          )}

          {/* Progress bar (when max_pages is known) */}
          {progressPercent !== null && (
            <Card>
              <CardContent className="py-4 space-y-2">
                <div className="flex items-center justify-between text-sm">
                  <span className="text-muted-foreground">Progress</span>
                  <span className="font-medium">
                    {status.pages_crawled}
                    {status.max_pages ? ` / ${status.max_pages}` : ""} pages
                    <span className="text-muted-foreground ml-2">
                      ({progressPercent}%)
                    </span>
                  </span>
                </div>
                <Progress value={progressPercent} className="h-2" />
                {status.eta_seconds != null && status.eta_seconds > 0 && (
                  <p className="text-xs text-muted-foreground">
                    ~{formatDuration(status.eta_seconds)} remaining
                  </p>
                )}
              </CardContent>
            </Card>
          )}

          {/* Stats grid */}
          <div className="grid gap-3 grid-cols-2 md:grid-cols-4">
            <Card>
              <CardContent className="py-4">
                <div className="flex items-center gap-2">
                  <FileText className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm text-muted-foreground">
                    Pages Crawled
                  </span>
                </div>
                <div className="text-2xl font-bold mt-1">
                  {status.pages_crawled}
                </div>
                {status.pages_indexed > 0 && (
                  <p className="text-xs text-muted-foreground">
                    {status.pages_indexed} indexed
                  </p>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardContent className="py-4">
                <div className="flex items-center gap-2">
                  <AlertCircle className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm text-muted-foreground">Errors</span>
                </div>
                <div
                  className={`text-2xl font-bold mt-1 ${
                    status.errors > 0 ? "text-destructive" : ""
                  }`}
                >
                  {status.errors}
                </div>
                {status.error_message && (
                  <p className="text-xs text-destructive truncate">
                    {status.error_message}
                  </p>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardContent className="py-4">
                <div className="flex items-center gap-2">
                  <Clock className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm text-muted-foreground">
                    Duration
                  </span>
                </div>
                <div className="text-2xl font-bold mt-1">
                  {status.duration_seconds != null
                    ? formatDuration(status.duration_seconds)
                    : "—"}
                </div>
                {status.started_at && (
                  <p className="text-xs text-muted-foreground">
                    Started{" "}
                    {formatDistanceToNow(new Date(status.started_at), {
                      addSuffix: true,
                    })}
                  </p>
                )}
              </CardContent>
            </Card>

            <Card>
              <CardContent className="py-4">
                <div className="flex items-center gap-2">
                  <Zap className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm text-muted-foreground">Speed</span>
                </div>
                <div className="text-2xl font-bold mt-1">
                  {status.crawl_rate > 0
                    ? `${status.crawl_rate.toFixed(1)}/s`
                    : "—"}
                </div>
                {status.documents_sent > 0 && (
                  <p className="text-xs text-muted-foreground">
                    {status.documents_sent} docs sent
                  </p>
                )}
              </CardContent>
            </Card>
          </div>

          {/* Extra info row */}
          <div className="grid gap-3 md:grid-cols-2">
            <Card>
              <CardContent className="py-4">
                <p className="text-sm text-muted-foreground">Index</p>
                <p className="font-mono text-sm mt-1">{status.index_uid}</p>
              </CardContent>
            </Card>
            {progressPercent === null && status.max_pages == null && (
              <Card>
                <CardContent className="py-4">
                  <p className="text-sm text-muted-foreground">Pages</p>
                  <p className="text-sm mt-1">
                    {status.pages_crawled} crawled, {status.documents_sent}{" "}
                    sent
                    {status.eta_seconds != null && status.eta_seconds > 0 && (
                      <span className="text-muted-foreground">
                        {" "}
                        &middot; ~{formatDuration(status.eta_seconds)} remaining
                      </span>
                    )}
                  </p>
                </CardContent>
              </Card>
            )}
          </div>

          {/* Crawl Configuration */}
          {status.config && (
            <Collapsible>
              <Card>
                <CollapsibleTrigger asChild>
                  <CardHeader className="group cursor-pointer select-none pb-3 hover:bg-muted/50 transition-colors">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <Settings2 className="h-4 w-4 text-muted-foreground" />
                        <CardTitle className="text-base">
                          Configuration
                        </CardTitle>
                      </div>
                      <ChevronDown className="h-4 w-4 text-muted-foreground transition-transform group-data-[state=open]:rotate-180" />
                    </div>
                    <CardDescription>
                      Crawl parameters used for this job
                    </CardDescription>
                  </CardHeader>
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <CardContent className="pt-0">
                    <ConfigSummary config={status.config} />
                  </CardContent>
                </CollapsibleContent>
              </Card>
            </Collapsible>
          )}
        </>
      )}

      {/* Live Event Log */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="text-base">Live Events</CardTitle>
              <CardDescription>Real-time crawl activity</CardDescription>
            </div>
            <div className="flex items-center gap-2">
              {logs.length > 0 && (
                <Badge variant="outline" className="text-xs">
                  {logs.length} events
                </Badge>
              )}
              <Badge variant={wsConnected ? "default" : "outline"}>
                <span
                  className={`mr-1.5 inline-block h-1.5 w-1.5 rounded-full ${
                    wsConnected ? "bg-green-400" : "bg-gray-400"
                  }`}
                />
                {wsConnected ? "Live" : "Disconnected"}
              </Badge>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          <div
            ref={logRef}
            className="h-[350px] bg-muted rounded-lg p-4 overflow-auto font-mono text-xs space-y-0.5"
          >
            {logs.length === 0 ? (
              <div className="flex items-center justify-center h-full text-muted-foreground">
                <p>Waiting for events...</p>
              </div>
            ) : (
              logs.map((log, i) => (
                <div
                  key={i}
                  className={`py-0.5 ${
                    log.variant === "error" ? "text-destructive" : ""
                  }`}
                >
                  <span className="text-muted-foreground select-none">
                    [{log.time}]
                  </span>{" "}
                  {log.message}
                </div>
              ))
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
