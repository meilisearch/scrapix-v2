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
} from "lucide-react";
import { fetchJobStatus, deleteJob, wsUrl } from "@/lib/api";
import type { JobStatus, WsMessage } from "@/lib/api-types";
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

      ws.onmessage = (event) => {
        try {
          const msg: WsMessage = JSON.parse(event.data);
          switch (msg.type) {
            case "page_crawled":
              addLog(
                `Crawled ${msg.url} (${msg.status}) in ${msg.elapsed_ms}ms`
              );
              setStatus((prev) =>
                prev
                  ? { ...prev, pages_crawled: prev.pages_crawled + 1 }
                  : prev
              );
              break;
            case "page_failed":
              addLog(`Failed ${msg.url}: ${msg.error}`, "error");
              setStatus((prev) =>
                prev ? { ...prev, errors: prev.errors + 1 } : prev
              );
              break;
            case "job_progress":
              setStatus((prev) =>
                prev
                  ? {
                      ...prev,
                      pages_crawled: msg.pages_crawled,
                      errors: msg.pages_failed,
                    }
                  : prev
              );
              break;
            case "job_completed":
              addLog(
                `Job completed: ${msg.pages_crawled} crawled, ${msg.pages_failed} failed`
              );
              setStatus((prev) =>
                prev ? { ...prev, status: "completed" } : prev
              );
              break;
            case "job_failed":
              addLog(`Job failed: ${msg.error}`, "error");
              setStatus((prev) =>
                prev ? { ...prev, status: "failed" } : prev
              );
              break;
          }
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
