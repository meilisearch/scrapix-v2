"use client";

import { useState, useMemo } from "react";
import Link from "next/link";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Skeleton } from "@/components/ui/skeleton";
import { Trash2, ExternalLink, AlertCircle, RefreshCw } from "lucide-react";
import { fetchJobs, deleteJob } from "@/lib/api";
import type { Job } from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";
import { cn } from "@/lib/utils";

const statusVariant: Record<string, "default" | "secondary" | "destructive" | "outline"> = {
  completed: "default",
  running: "secondary",
  pending: "outline",
  failed: "destructive",
  cancelled: "destructive",
};

const STATUS_TABS = ["all", "running", "completed", "failed"] as const;
type StatusTab = (typeof STATUS_TABS)[number];

function getHostname(url: string): string | null {
  try {
    return new URL(url).hostname;
  } catch {
    return null;
  }
}

function jobLabel(job: Job): string {
  if (job.start_urls && job.start_urls.length > 0) {
    return getHostname(job.start_urls[0]) ?? job.index_uid;
  }
  return job.index_uid;
}

export default function JobsPage() {
  const queryClient = useQueryClient();
  const [statusFilter, setStatusFilter] = useState<StatusTab>("all");
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  const {
    data: jobs = [],
    isLoading,
    isFetching,
    error,
    dataUpdatedAt,
  } = useQuery({
    queryKey: ["jobs"],
    queryFn: fetchJobs,
    refetchInterval: 5_000,
  });

  const handleManualRefresh = () => {
    queryClient.invalidateQueries({ queryKey: ["jobs"] });
  };

  const countByStatus = useMemo(() => {
    const counts: Record<string, number> = { all: jobs.length, running: 0, completed: 0, failed: 0 };
    for (const job of jobs) {
      if (job.status in counts) counts[job.status]++;
      if (job.status === "cancelled") counts.failed++;
    }
    return counts;
  }, [jobs]);

  const filteredJobs = useMemo(() => {
    if (statusFilter === "all") return jobs;
    if (statusFilter === "failed") return jobs.filter((j) => j.status === "failed" || j.status === "cancelled");
    return jobs.filter((j) => j.status === statusFilter);
  }, [jobs, statusFilter]);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      await deleteJob(deleteTarget);
      queryClient.invalidateQueries({ queryKey: ["jobs"] });
      toast.success("Job deleted");
    } catch {
      toast.error("Failed to delete job");
    } finally {
      setDeleteTarget(null);
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Jobs</h2>
        <p className="text-muted-foreground">
          View and manage your crawl jobs
        </p>
      </div>

      {error && (
        <Card className="border-destructive">
          <CardContent className="py-4 flex items-center gap-3">
            <AlertCircle className="h-5 w-5 text-destructive" />
            <p className="text-sm text-destructive">
              Could not reach the Scrapix API: {error instanceof Error ? error.message : "Unknown error"}
            </p>
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <CardTitle>Crawl Jobs</CardTitle>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              {dataUpdatedAt > 0 && (
                <span>
                  Updated {formatDistanceToNow(new Date(dataUpdatedAt), { addSuffix: true })}
                </span>
              )}
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={handleManualRefresh}
                disabled={isFetching}
              >
                <RefreshCw className={cn("h-3.5 w-3.5", isFetching && "animate-spin")} />
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {!isLoading && jobs.length > 0 && (
            <Tabs value={statusFilter} onValueChange={(v) => setStatusFilter(v as StatusTab)}>
              <TabsList>
                {STATUS_TABS.map((tab) => (
                  <TabsTrigger key={tab} value={tab} className="capitalize gap-1.5">
                    {tab}
                    <Badge variant="outline" className="ml-1 h-5 min-w-5 px-1 text-[10px]">
                      {countByStatus[tab] ?? 0}
                    </Badge>
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
          )}

          {isLoading ? (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Job</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Index</TableHead>
                  <TableHead>Progress</TableHead>
                  <TableHead>Started</TableHead>
                  <TableHead className="w-[100px]">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {Array.from({ length: 5 }).map((_, i) => (
                  <TableRow key={i}>
                    <TableCell>
                      <Skeleton className="h-4 w-32" />
                      <Skeleton className="h-3 w-16 mt-1" />
                    </TableCell>
                    <TableCell><Skeleton className="h-5 w-16 rounded-full" /></TableCell>
                    <TableCell><Skeleton className="h-4 w-24" /></TableCell>
                    <TableCell><Skeleton className="h-4 w-28" /></TableCell>
                    <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                    <TableCell><Skeleton className="h-8 w-16" /></TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          ) : filteredJobs.length === 0 ? (
            <p className="text-sm text-muted-foreground py-8 text-center">
              {jobs.length === 0 ? (
                <>
                  No jobs yet. Start a crawl from the{" "}
                  <Link href="/playground" className="text-primary hover:underline">
                    Playground
                  </Link>
                  .
                </>
              ) : (
                <>No {statusFilter} jobs.</>
              )}
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Job</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Index</TableHead>
                  <TableHead>Progress</TableHead>
                  <TableHead>Started</TableHead>
                  <TableHead className="w-[100px]">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredJobs.map((job) => {
                  const isRunning = job.status === "running";
                  const progressPercent =
                    isRunning && job.max_pages && job.max_pages > 0
                      ? Math.min(100, Math.round((job.pages_crawled / job.max_pages) * 100))
                      : null;

                  return (
                    <TableRow key={job.job_id}>
                      <TableCell>
                        <Link
                          href={`/jobs/${job.job_id}`}
                          className="hover:underline text-primary font-medium text-sm"
                        >
                          {jobLabel(job)}
                        </Link>
                        <p className="font-mono text-[11px] text-muted-foreground">
                          {job.job_id.slice(0, 8)}...
                        </p>
                      </TableCell>
                      <TableCell>
                        <Badge variant={statusVariant[job.status] || "outline"}>
                          {isRunning && (
                            <span className="relative mr-1.5 flex h-2 w-2">
                              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-current opacity-75" />
                              <span className="relative inline-flex h-2 w-2 rounded-full bg-current" />
                            </span>
                          )}
                          {job.status}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-sm max-w-[200px] truncate">
                        {job.index_uid}
                      </TableCell>
                      <TableCell>
                        {isRunning ? (
                          <div className="space-y-1">
                            {progressPercent !== null ? (
                              <div className="flex items-center gap-2">
                                <Progress value={progressPercent} className="h-1.5 w-20" />
                                <span className="text-xs text-muted-foreground">
                                  {job.pages_crawled}/{job.max_pages}
                                </span>
                              </div>
                            ) : (
                              <span className="text-sm">
                                {job.pages_crawled} pages
                              </span>
                            )}
                            {job.crawl_rate > 0 && (
                              <p className="text-[11px] text-muted-foreground">
                                {job.crawl_rate.toFixed(1)}/s
                              </p>
                            )}
                          </div>
                        ) : (
                          <div>
                            <span className="text-sm">
                              {job.pages_crawled} pages
                            </span>
                            {job.errors > 0 && (
                              <span className="text-xs text-destructive ml-2">
                                {job.errors} errors
                              </span>
                            )}
                          </div>
                        )}
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        {job.started_at
                          ? formatDistanceToNow(new Date(job.started_at), {
                              addSuffix: true,
                            })
                          : "—"}
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1">
                          <Button variant="ghost" size="icon" asChild>
                            <Link href={`/jobs/${job.job_id}`}>
                              <ExternalLink className="h-4 w-4" />
                            </Link>
                          </Button>
                          <Button
                            variant="ghost"
                            size="icon"
                            onClick={() => setDeleteTarget(job.job_id)}
                          >
                            <Trash2 className="h-4 w-4 text-destructive" />
                          </Button>
                        </div>
                      </TableCell>
                    </TableRow>
                  );
                })}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {/* Delete confirmation dialog */}
      <Dialog open={deleteTarget !== null} onOpenChange={(open) => { if (!open) setDeleteTarget(null); }}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Job</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete job{" "}
              <span className="font-mono">{deleteTarget?.slice(0, 8)}...</span>?
              This action is permanent and cannot be undone.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
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
