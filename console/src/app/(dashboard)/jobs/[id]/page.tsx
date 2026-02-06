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
import { ArrowLeft, Trash2, AlertCircle } from "lucide-react";
import { fetchJobStatus, deleteJob, wsUrl } from "@/lib/api";
import type { JobStatus, WsMessage } from "@/lib/api-types";
import { toast } from "sonner";

interface LogEntry {
  time: string;
  message: string;
  variant: "default" | "error";
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

  const addLog = useCallback((message: string, variant: "default" | "error" = "default") => {
    setLogs((prev) => {
      const next = [
        ...prev,
        { time: new Date().toLocaleTimeString(), message, variant },
      ];
      // Keep last 200 entries
      return next.length > 200 ? next.slice(-200) : next;
    });
  }, []);

  // Fetch initial status
  useEffect(() => {
    if (!id) return;
    fetchJobStatus(id)
      .then((data) => {
        setStatus(data);
        setApiError(null);
      })
      .catch((e) =>
        setApiError(e instanceof Error ? e.message : "Failed to fetch job")
      );
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
              addLog(`Crawled ${msg.url} (${msg.status}) in ${msg.elapsed_ms}ms`);
              setStatus((prev) =>
                prev ? { ...prev, pages_crawled: prev.pages_crawled + 1 } : prev
              );
              break;
            case "page_failed":
              addLog(`Failed ${msg.url}: ${msg.error}`, "error");
              setStatus((prev) =>
                prev ? { ...prev, pages_failed: prev.pages_failed + 1 } : prev
              );
              break;
            case "job_progress":
              setStatus((prev) =>
                prev
                  ? {
                      ...prev,
                      pages_crawled: msg.pages_crawled,
                      pages_failed: msg.pages_failed,
                      pages_total: msg.pages_total ?? prev.pages_total,
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
        // Reconnect after 3 seconds unless component unmounted
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

  const progress =
    status?.pages_total && status.pages_total > 0
      ? Math.round(
          ((status.pages_crawled + status.pages_failed) / status.pages_total) *
            100
        )
      : null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" asChild>
            <Link href="/jobs">
              <ArrowLeft className="h-4 w-4" />
            </Link>
          </Button>
          <div>
            <h2 className="text-2xl font-bold tracking-tight">Job Detail</h2>
            <p className="text-muted-foreground font-mono text-sm">{id}</p>
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
          {/* Status Overview */}
          <div className="grid gap-4 md:grid-cols-4">
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">Status</CardTitle>
              </CardHeader>
              <CardContent>
                <Badge
                  variant={
                    status.status === "completed"
                      ? "default"
                      : status.status === "running"
                      ? "secondary"
                      : status.status === "failed"
                      ? "destructive"
                      : "outline"
                  }
                  className="text-base"
                >
                  {status.status}
                </Badge>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">
                  Pages Crawled
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">
                  {status.pages_crawled}
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">
                  Pages Failed
                </CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold text-destructive">
                  {status.pages_failed}
                </div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">Elapsed</CardTitle>
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">
                  {status.elapsed_seconds != null
                    ? `${status.elapsed_seconds}s`
                    : "-"}
                </div>
              </CardContent>
            </Card>
          </div>

          {/* Progress Bar */}
          {progress != null && (
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">Progress</CardTitle>
                <CardDescription>
                  {status.pages_crawled + status.pages_failed} /{" "}
                  {status.pages_total} pages
                </CardDescription>
              </CardHeader>
              <CardContent>
                <div className="w-full bg-muted rounded-full h-3">
                  <div
                    className="bg-primary h-3 rounded-full transition-all duration-300"
                    style={{ width: `${Math.min(progress, 100)}%` }}
                  />
                </div>
                <p className="text-xs text-muted-foreground mt-1">
                  {progress}%
                </p>
              </CardContent>
            </Card>
          )}

          {/* Extra info */}
          {(status.current_depth != null || status.urls_in_queue != null) && (
            <div className="grid gap-4 md:grid-cols-2">
              {status.current_depth != null && (
                <Card>
                  <CardHeader className="pb-2">
                    <CardTitle className="text-sm font-medium">
                      Current Depth
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <div className="text-2xl font-bold">
                      {status.current_depth}
                    </div>
                  </CardContent>
                </Card>
              )}
              {status.urls_in_queue != null && (
                <Card>
                  <CardHeader className="pb-2">
                    <CardTitle className="text-sm font-medium">
                      URLs in Queue
                    </CardTitle>
                  </CardHeader>
                  <CardContent>
                    <div className="text-2xl font-bold">
                      {status.urls_in_queue}
                    </div>
                  </CardContent>
                </Card>
              )}
            </div>
          )}
        </>
      )}

      {/* Live Event Log */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle>Live Events</CardTitle>
              <CardDescription>Real-time crawl activity</CardDescription>
            </div>
            <Badge variant={wsConnected ? "default" : "outline"}>
              {wsConnected ? "Connected" : "Disconnected"}
            </Badge>
          </div>
        </CardHeader>
        <CardContent>
          <div
            ref={logRef}
            className="h-[300px] bg-muted rounded-lg p-4 overflow-auto font-mono text-xs space-y-1"
          >
            {logs.length === 0 ? (
              <p className="text-muted-foreground">
                Waiting for events...
              </p>
            ) : (
              logs.map((log, i) => (
                <div
                  key={i}
                  className={
                    log.variant === "error" ? "text-destructive" : ""
                  }
                >
                  <span className="text-muted-foreground">[{log.time}]</span>{" "}
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
