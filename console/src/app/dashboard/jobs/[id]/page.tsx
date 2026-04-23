"use client";

import { useEffect, useState, useRef, useCallback, useMemo } from "react";
import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Input } from "@/components/ui/input";
import {
  ArrowLeft,
  Trash2,
  AlertCircle,
  ExternalLink,
  Settings2,
  MoreVertical,
  Eraser,
  RotateCw,
  X,
} from "lucide-react";
import {
  fetchJobStatus,
  deleteJob,
  createCrawl,
  wsUrl,
  fetchJobEventHistory,
} from "@/lib/api";
import type { JobStatus, WsServerMessage, CrawlEvent } from "@/lib/api-types";
import { toast } from "sonner";
import { formatDistanceToNow } from "date-fns";
import { cn } from "@/lib/utils";

type LogCategory = "crawled" | "indexed" | "error" | "info";

interface LogEntry {
  time: string;
  message: string;
  variant: "default" | "error";
  category: LogCategory;
}

const LOG_TABS = ["all", "crawled", "indexed", "errors"] as const;
type LogTab = (typeof LOG_TABS)[number];

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

function getHostname(url: string): string | null {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function ConfigSummary({ config }: { config: Record<string, any> }) {
  const mainFields: [string, string][] = [];
  const meilisearch: [string, string][] = [];
  const patterns: [string, string][] = [];
  const other: [string, string][] = [];

  const skip = new Set(["start_urls", "index_uid", "max_pages"]);

  const formatValue = (v: unknown): string => {
    if (v == null) return "—";
    if (typeof v === "boolean") return v ? "Yes" : "No";
    if (Array.isArray(v)) return v.length === 0 ? "—" : v.join(", ");
    if (typeof v === "object") return JSON.stringify(v);
    return String(v);
  };

  const labelMap: Record<string, string> = {
    max_depth: "Max Depth",
    crawler_type: "Crawler Type",
    allowed_domains: "Allowed Domains",
  };

  for (const [key, value] of Object.entries(config)) {
    if (skip.has(key)) continue;
    if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      const entries = Object.entries(value).filter(
        ([, v]) =>
          v != null &&
          v !== "" &&
          v !== false &&
          !(Array.isArray(v) && v.length === 0),
      );
      if (entries.length === 0) continue;

      if (key === "meilisearch") {
        for (const [k, v] of entries) {
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

    if (
      key in labelMap ||
      ["max_depth", "crawler_type", "allowed_domains"].includes(key)
    ) {
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

function ConfigRow({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="grid grid-cols-[auto_1fr] gap-x-6 items-start">
      <span className="text-sm text-muted-foreground whitespace-nowrap">
        {label}
      </span>
      <span className="text-sm font-mono break-all">{children}</span>
    </div>
  );
}

function JobConfigPanel({ status }: { status: JobStatus }) {
  return (
    <Card className="h-full">
      <CardHeader className="pb-3">
        <div className="flex items-center gap-2">
          <Settings2 className="h-4 w-4 text-muted-foreground" />
          <CardTitle className="text-base">Configuration</CardTitle>
        </div>
        <CardDescription>Parameters used to start this job</CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <ConfigRow label="Index">{status.index_uid}</ConfigRow>

        {status.start_urls && status.start_urls.length > 0 && (
          <div className="grid grid-cols-[auto_1fr] gap-x-6 items-start">
            <span className="text-sm text-muted-foreground whitespace-nowrap">
              Start URLs
            </span>
            <div className="space-y-1">
              {status.start_urls.map((url) => (
                <div key={url} className="flex items-center gap-1">
                  <span className="text-sm font-mono break-all">{url}</span>
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
        )}

        {status.config && <ConfigSummary config={status.config} />}

        <div className="border-t pt-3 space-y-1">
          <ConfigRow label="Speed">
            {status.crawl_rate > 0
              ? `${status.crawl_rate.toFixed(1)} pages/s`
              : "—"}
          </ConfigRow>
          <ConfigRow label="Pages indexed">
            {status.pages_indexed} / {status.documents_sent} sent
          </ConfigRow>
        </div>
      </CardContent>
    </Card>
  );
}

export default function JobDetailPage() {
  const { id } = useParams<{ id: string }>();
  const router = useRouter();
  const queryClient = useQueryClient();
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [wsConnected, setWsConnected] = useState(false);
  const [logFilter, setLogFilter] = useState<LogTab>("all");
  const [logSearch, setLogSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [elapsedSeconds, setElapsedSeconds] = useState<number | null>(null);
  const logRef = useRef<HTMLDivElement>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const historyAppliedRef = useRef(false);

  const { data: status, error: apiError } = useQuery({
    queryKey: ["job", id],
    queryFn: () => fetchJobStatus(id),
    enabled: !!id,
    refetchInterval: 3_000,
  });

  useEffect(() => {
    if (!status?.started_at) return;

    const computeElapsed = () => {
      const start = new Date(status.started_at!).getTime();
      const end = status.completed_at
        ? new Date(status.completed_at).getTime()
        : Date.now();
      setElapsedSeconds(Math.floor((end - start) / 1000));
    };

    computeElapsed();

    if (status.status !== "running") return;

    const interval = setInterval(computeElapsed, 1000);
    return () => clearInterval(interval);
  }, [status?.started_at, status?.completed_at, status?.status]);

  useEffect(() => {
    const t = setTimeout(() => setDebouncedSearch(logSearch), 150);
    return () => clearTimeout(t);
  }, [logSearch]);

  // Fetch event history for completed/failed jobs (from ClickHouse)
  const isTerminal =
    status?.status === "completed" || status?.status === "failed";
  const { data: eventHistory, isLoading: historyLoading } = useQuery({
    queryKey: ["job-events-history", id],
    queryFn: () => fetchJobEventHistory(id as string),
    enabled: !!id && isTerminal,
    staleTime: Infinity,
  });

  const addLog = useCallback(
    (
      message: string,
      variant: "default" | "error" = "default",
      category: LogCategory = "info",
    ) => {
      setLogs((prev) => {
        const next = [
          ...prev,
          { time: new Date().toLocaleTimeString(), message, variant, category },
        ];
        return next.length > 5000 ? next.slice(-5000) : next;
      });
    },
    [],
  );

  // WebSocket connection
  useEffect(() => {
    if (!id) return;

    let reconnectTimer: ReturnType<typeof setTimeout>;
    let aborted = false;

    function connect() {
      if (aborted) return;
      const ws = new WebSocket(wsUrl(`/ws/job/${id}`));
      wsRef.current = ws;

      ws.onopen = () => {
        setWsConnected(true);
        addLog("Connected to live feed", "default", "info");
      };

      ws.onmessage = (rawEvent) => {
        try {
          const msg: WsServerMessage = JSON.parse(rawEvent.data);

          if (msg.type === "status") {
            queryClient.setQueryData(["job", id], msg.status);
            addLog(
              `Status: ${msg.status.status} — ${msg.status.pages_crawled} crawled, ${msg.status.pages_indexed} indexed, ${msg.status.errors} errors`,
              "default",
              "info",
            );
            return;
          }

          if (msg.type === "event") {
            const evt: CrawlEvent = msg.event;
            switch (evt.type) {
              case "page_crawled":
                addLog(
                  `Crawled ${evt.url} (${evt.status}) in ${evt.duration_ms}ms`,
                  "default",
                  "crawled",
                );
                queryClient.setQueryData(
                  ["job", id],
                  (prev: JobStatus | undefined) =>
                    prev
                      ? { ...prev, pages_crawled: prev.pages_crawled + 1 }
                      : prev,
                );
                break;
              case "page_failed":
                addLog(`Failed ${evt.url}: ${evt.error}`, "error", "error");
                queryClient.setQueryData(
                  ["job", id],
                  (prev: JobStatus | undefined) =>
                    prev ? { ...prev, errors: prev.errors + 1 } : prev,
                );
                break;
              case "document_indexed":
                addLog(
                  `Indexed ${evt.url} → ${evt.document_id}`,
                  "default",
                  "indexed",
                );
                queryClient.setQueryData(
                  ["job", id],
                  (prev: JobStatus | undefined) =>
                    prev
                      ? {
                          ...prev,
                          pages_indexed: prev.pages_indexed + 1,
                          documents_sent: prev.documents_sent + 1,
                        }
                      : prev,
                );
                break;
              case "urls_discovered":
                addLog(
                  `Discovered ${evt.count} URLs from ${evt.source_url}`,
                  "default",
                  "crawled",
                );
                break;
              case "page_skipped":
                addLog(`Skipped ${evt.url}: ${evt.reason}`, "default", "info");
                break;
              case "rate_limited":
                addLog(
                  `Rate limited on ${evt.domain}, waiting ${evt.wait_ms}ms`,
                  "default",
                  "info",
                );
                break;
              case "job_started":
                addLog(
                  `Job started: crawling ${evt.start_urls.length} seed URL(s)`,
                  "default",
                  "info",
                );
                queryClient.setQueryData(
                  ["job", id],
                  (prev: JobStatus | undefined) =>
                    prev ? { ...prev, status: "running" } : prev,
                );
                break;
              case "job_completed":
                addLog(
                  `Job completed: ${evt.pages_crawled} crawled, ${evt.documents_indexed} indexed, ${evt.errors} errors in ${evt.duration_secs}s`,
                  "default",
                  "info",
                );
                queryClient.setQueryData(
                  ["job", id],
                  (prev: JobStatus | undefined) =>
                    prev ? { ...prev, status: "completed", completed_at: new Date().toISOString() } : prev,
                );
                break;
              case "job_failed":
                addLog(`Job failed: ${evt.error}`, "error", "error");
                queryClient.setQueryData(
                  ["job", id],
                  (prev: JobStatus | undefined) =>
                    prev ? { ...prev, status: "failed" } : prev,
                );
                break;
            }
            return;
          }
        } catch {
          // ignore parse errors
        }
      };

      ws.onclose = () => {
        setWsConnected(false);
        if (!aborted) {
          reconnectTimer = setTimeout(connect, 3000);
        }
      };

      ws.onerror = () => {
        ws.close();
      };
    }

    connect();

    return () => {
      aborted = true;
      clearTimeout(reconnectTimer);
      wsRef.current?.close();
    };
  }, [id, addLog, queryClient]);

  // When event history loads for a completed/failed job, populate the log
  useEffect(() => {
    if (!eventHistory || historyAppliedRef.current) return;
    historyAppliedRef.current = true;

    for (const evt of eventHistory.events) {
      switch (evt.event_type) {
        case "page_crawled":
          addLog(
            `Crawled ${evt.url} (${evt.status_code}) in ${evt.duration_ms}ms`,
            "default",
            "crawled",
          );
          break;
        case "page_failed":
          addLog(`Failed ${evt.url}: ${evt.error}`, "error", "error");
          break;
        case "document_indexed":
          addLog(
            `Indexed ${evt.url} → ${evt.document_id}`,
            "default",
            "indexed",
          );
          break;
        case "urls_discovered":
          addLog(
            `Discovered ${evt.urls_count} URLs from ${evt.source_url}`,
            "default",
            "crawled",
          );
          break;
        case "page_skipped":
          addLog(`Skipped ${evt.url}: ${evt.reason}`, "default", "info");
          break;
        case "rate_limited":
          addLog(
            `Rate limited on ${evt.domain}, waiting ${evt.wait_ms}ms`,
            "default",
            "info",
          );
          break;
      }
    }
  }, [eventHistory, addLog]);

  // Auto-scroll logs
  useEffect(() => {
    const viewport = logRef.current?.parentElement;
    if (viewport) viewport.scrollTop = viewport.scrollHeight;
  }, [logs]);

  const handleDelete = async () => {
    if (!id) return;
    try {
      await deleteJob(id);
      toast.success("Job deleted");
      queryClient.invalidateQueries({ queryKey: ["jobs"] });
      router.push("/dashboard/jobs");
    } catch {
      toast.error("Failed to delete job");
    } finally {
      setDeleteOpen(false);
    }
  };

  const handleRetry = async () => {
    if (!status?.config) return;
    try {
      const { job_id } = await createCrawl(status.config);
      toast.success(`New job created: ${job_id.slice(0, 8)}...`);
      queryClient.invalidateQueries({ queryKey: ["jobs"] });
      router.push(`/dashboard/jobs/${job_id}`);
    } catch {
      toast.error("Failed to retry job");
    }
  };

  const formatDuration = (seconds: number) => {
    if (seconds < 60) return `${seconds}s`;
    const m = Math.floor(seconds / 60);
    const s = seconds % 60;
    return `${m}m ${s}s`;
  };

  const isRunning = status?.status === "running";

  const hostname = status?.start_urls?.[0]
    ? getHostname(status.start_urls[0])
    : null;

  const filteredLogs = useMemo(() => {
    let result = logs;
    if (logFilter === "errors")
      result = result.filter((l) => l.category === "error");
    else if (logFilter !== "all")
      result = result.filter((l) => l.category === logFilter);
    if (debouncedSearch.trim()) {
      const q = debouncedSearch.toLowerCase();
      result = result.filter((l) => l.message.toLowerCase().includes(q));
    }
    return result;
  }, [logs, logFilter, debouncedSearch]);

  return (
    <div className="space-y-4">
      {/* Unified header */}
      <div className="flex items-start justify-between">
        <div className="flex items-start gap-3">
          <Button variant="ghost" size="icon" className="mt-0.5" asChild>
            <Link href="/dashboard/jobs">
              <ArrowLeft className="h-4 w-4" />
            </Link>
          </Button>
          <div className="space-y-1">
            <div className="flex items-center gap-3">
              <h2 className="text-2xl font-bold tracking-tight">
                {hostname ?? "Job Detail"}
              </h2>
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
            <div className="flex items-center gap-2 text-muted-foreground">
              <p className="font-mono text-xs">{id}</p>
            </div>
          </div>
        </div>

        {/* Actions dropdown */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" size="icon">
              <MoreVertical className="h-4 w-4" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            {status?.start_urls?.[0] && (
              <>
                <DropdownMenuItem asChild>
                  <a
                    href={status.start_urls[0]}
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    <ExternalLink className="mr-2 h-4 w-4" />
                    Open URL
                  </a>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
              </>
            )}
            <DropdownMenuItem
              disabled={isRunning || !status?.config}
              onClick={handleRetry}
            >
              <RotateCw className="mr-2 h-4 w-4" />
              Retry job
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              className="text-destructive focus:text-destructive"
              onClick={() => setDeleteOpen(true)}
            >
              <Trash2 className="mr-2 h-4 w-4" />
              Delete job
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {apiError && (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>
            {apiError instanceof Error
              ? apiError.message
              : "Failed to fetch job"}
          </AlertDescription>
        </Alert>
      )}

      {status && (
        <>
          {status.status === "failed" && status.error_message && (
            <Alert variant="destructive">
              <AlertCircle className="h-4 w-4" />
              <AlertDescription>{status.error_message}</AlertDescription>
            </Alert>
          )}
          {/* Stats grid */}
          <div className="grid gap-3 grid-cols-2 md:grid-cols-3">
            <Card className="border-t-2 border-t-primary">
              <CardContent className="py-4">
                <span className="text-sm text-muted-foreground">
                  Pages Crawled
                </span>
                <div className="flex items-baseline gap-2 mt-1">
                  <span className="text-2xl font-bold">
                    {status.pages_crawled}
                  </span>
                  {status.pages_indexed > 0 && (
                    <span className="text-xs text-muted-foreground">
                      {status.pages_indexed} indexed
                    </span>
                  )}
                </div>
              </CardContent>
            </Card>

            <Card
              className={cn(
                "border-t-2",
                status.errors > 0 ? "border-t-destructive" : "border-t-muted",
              )}
            >
              <CardContent className="py-4">
                <span className="text-sm text-muted-foreground">Errors</span>
                <div className="flex items-baseline gap-2 mt-1">
                  <span
                    className={cn(
                      "text-2xl font-bold",
                      status.errors > 0 && "text-destructive",
                    )}
                  >
                    {status.errors}
                  </span>
                </div>
              </CardContent>
            </Card>

            <Card className="border-t-2 border-t-muted">
              <CardContent className="py-4">
                <span className="text-sm text-muted-foreground">Duration</span>
                <div className="flex items-baseline gap-2 mt-1">
                  <span className="text-2xl font-bold">
                    {status.started_at && elapsedSeconds !== null
                      ? formatDuration(elapsedSeconds)
                      : "—"}
                  </span>
                  {status.started_at && (
                    <span className="text-xs text-muted-foreground">
                      {formatDistanceToNow(new Date(status.started_at), {
                        addSuffix: true,
                      })}
                    </span>
                  )}
                </div>
              </CardContent>
            </Card>
          </div>

          <div className="grid gap-4 lg:grid-cols-[2fr_3fr] items-start">
            <JobConfigPanel status={status} />

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
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 text-xs gap-1"
                        onClick={() => setLogs([])}
                      >
                        <Eraser className="h-3 w-3" />
                        Clear
                      </Button>
                    )}
                    {logs.length > 0 && (
                      <Badge variant="outline" className="text-xs">
                        {filteredLogs.length} events
                      </Badge>
                    )}
                    <Badge variant={wsConnected ? "default" : "outline"}>
                      <span
                        className={cn(
                          "mr-1.5 inline-block h-1.5 w-1.5 rounded-full",
                          wsConnected ? "bg-green-400" : "bg-gray-400",
                        )}
                      />
                      {wsConnected ? "Live" : "Disconnected"}
                    </Badge>
                  </div>
                </div>

                {/* Search bar */}
                <div className="relative mt-2">
                  <Input
                    placeholder="Search events…"
                    value={logSearch}
                    onChange={(e) => setLogSearch(e.target.value)}
                    className="h-8 text-xs pr-7"
                  />
                  {logSearch && (
                    <button
                      type="button"
                      aria-label="Clear search"
                      onClick={() => setLogSearch("")}
                      className="absolute right-2 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                    >
                      <X className="h-3 w-3" />
                    </button>
                  )}
                </div>

                <Tabs
                  value={logFilter}
                  onValueChange={(v) => setLogFilter(v as LogTab)}
                  className="mt-2"
                >
                  <TabsList className="h-8">
                    <TabsTrigger value="all" className="text-xs h-7 px-2.5">
                      All
                    </TabsTrigger>
                    <TabsTrigger value="crawled" className="text-xs h-7 px-2.5">
                      Crawled
                    </TabsTrigger>
                    <TabsTrigger value="indexed" className="text-xs h-7 px-2.5">
                      Indexed
                    </TabsTrigger>
                    <TabsTrigger value="errors" className="text-xs h-7 px-2.5">
                      Errors
                    </TabsTrigger>
                  </TabsList>
                </Tabs>
              </CardHeader>
              <CardContent>
                <ScrollArea className="h-[350px] rounded-lg bg-muted">
                  <div
                    ref={logRef}
                    className="p-4 font-mono text-xs space-y-0.5"
                  >
                    {filteredLogs.length === 0 ? (
                      <div className="flex items-center justify-center h-[318px] text-muted-foreground">
                        <p>
                          {isTerminal && historyLoading
                            ? "Loading event history..."
                            : logs.length === 0
                              ? "Waiting for events..."
                              : "No matching events"}
                        </p>
                      </div>
                    ) : (
                      filteredLogs.map((log, i) => (
                        <div
                          key={i}
                          className={cn(
                            "py-0.5",
                            log.variant === "error" && "text-destructive",
                          )}
                        >
                          <span className="text-muted-foreground select-none">
                            [{log.time}]
                          </span>{" "}
                          {log.message}
                        </div>
                      ))
                    )}
                  </div>
                </ScrollArea>
              </CardContent>
            </Card>
          </div>
        </>
      )}

      {/* Delete confirmation dialog */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Job</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete job{" "}
              <span className="font-mono">{id?.slice(0, 8)}...</span>? This
              action is permanent and cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleDelete}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
