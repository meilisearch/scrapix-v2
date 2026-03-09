"use client";

import { useState, useMemo } from "react";
import Link from "next/link";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
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
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import {
  Pagination,
  PaginationContent,
  PaginationItem,
  PaginationNext,
  PaginationPrevious,
} from "@/components/ui/pagination";
import {
  Trash2,
  ExternalLink,
  AlertCircle,
  Search,
  Plus,
} from "lucide-react";
import { TableSkeleton } from "@/components/table-skeleton";
import { EmptyState } from "@/components/empty-state";
import { fetchJobs, deleteJob } from "@/lib/api";
import type { Job } from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const statusVariant: Record<
  string,
  "default" | "secondary" | "destructive" | "outline"
> = {
  completed: "default",
  running: "secondary",
  pending: "outline",
  failed: "destructive",
  cancelled: "outline",
};

const STATUS_TABS = [
  "all",
  "running",
  "completed",
  "failed",
  "cancelled",
] as const;
type StatusTab = (typeof STATUS_TABS)[number];

const PAGE_SIZE = 20;

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
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null);

  const {
    data: jobs = [],
    isLoading,
    error,
  } = useQuery({
    queryKey: ["jobs"],
    queryFn: fetchJobs,
    refetchInterval: 5_000,
  });

  const countByStatus = useMemo(() => {
    const counts: Record<string, number> = {
      all: jobs.length,
      running: 0,
      completed: 0,
      failed: 0,
      cancelled: 0,
    };
    for (const job of jobs) {
      if (job.status in counts) counts[job.status]++;
    }
    return counts;
  }, [jobs]);

  const filteredJobs = useMemo(() => {
    let result = jobs;

    if (statusFilter !== "all") {
      result = result.filter((j) => j.status === statusFilter);
    }

    if (search.trim()) {
      const q = search.toLowerCase();
      result = result.filter(
        (j) =>
          j.job_id.toLowerCase().includes(q) ||
          j.index_uid.toLowerCase().includes(q) ||
          jobLabel(j).toLowerCase().includes(q) ||
          j.start_urls?.some((u) => u.toLowerCase().includes(q))
      );
    }

    return result;
  }, [jobs, statusFilter, search]);

  // Reset page when filters change
  const totalPages = Math.max(1, Math.ceil(filteredJobs.length / PAGE_SIZE));
  const safePage = Math.min(page, totalPages - 1);
  const pageJobs = filteredJobs.slice(
    safePage * PAGE_SIZE,
    (safePage + 1) * PAGE_SIZE
  );

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

  const deleteLabel = deleteTarget
    ? jobLabel(jobs.find((j) => j.job_id === deleteTarget) ?? { job_id: deleteTarget, index_uid: deleteTarget } as Job)
    : "";

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Jobs</h2>
          <p className="text-muted-foreground">
            View and manage your crawl jobs
          </p>
        </div>
        <div className="flex items-center gap-3">
          {!isLoading && jobs.length > 0 && (
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search jobs..."
                value={search}
                onChange={(e) => {
                  setSearch(e.target.value);
                  setPage(0);
                }}
                className="pl-9 h-9 w-[200px]"
              />
            </div>
          )}
          <Button asChild>
            <Link href="/playground">
              <Plus className="h-4 w-4 mr-2" />
              New Crawl
            </Link>
          </Button>
        </div>
      </div>

      {error && (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>
            Could not reach the Scrapix API:{" "}
            {error instanceof Error ? error.message : "Unknown error"}
          </AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="pt-6 space-y-4">
          {!isLoading && jobs.length > 0 && (
            <Tabs
              value={statusFilter}
              onValueChange={(v) => {
                setStatusFilter(v as StatusTab);
                setPage(0);
              }}
            >
              <TabsList>
                {STATUS_TABS.map((tab) => {
                  const count = countByStatus[tab] ?? 0;
                  if (tab !== "all" && count === 0) return null;
                  return (
                    <TabsTrigger
                      key={tab}
                      value={tab}
                      className="capitalize gap-1.5"
                    >
                      {tab}
                      <Badge
                        variant="outline"
                        className="ml-1 h-5 min-w-5 px-1 text-[10px]"
                      >
                        {count}
                      </Badge>
                    </TabsTrigger>
                  );
                })}
              </TabsList>
            </Tabs>
          )}

          {isLoading ? (
            <TableSkeleton />
          ) : filteredJobs.length === 0 ? (
            jobs.length === 0 ? (
              <EmptyState
                message="No jobs yet"
                action={
                  <Button asChild variant="outline">
                    <Link href="/playground">
                      <Plus className="h-4 w-4 mr-2" />
                      Start your first crawl
                    </Link>
                  </Button>
                }
              />
            ) : (
              <EmptyState message="No matching jobs found." />
            )
          ) : (
            <>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Job</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead className="hidden md:table-cell">
                      Index
                    </TableHead>
                    <TableHead>Progress</TableHead>
                    <TableHead className="hidden sm:table-cell">
                      Started
                    </TableHead>
                    <TableHead className="w-[80px]" />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {pageJobs.map((job) => {
                    const isRunning = job.status === "running";
                    const progressPercent =
                      isRunning && job.max_pages && job.max_pages > 0
                        ? Math.min(
                            100,
                            Math.round(
                              (job.pages_crawled / job.max_pages) * 100
                            )
                          )
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
                            {job.job_id.slice(0, 8)}
                          </p>
                        </TableCell>
                        <TableCell>
                          <Badge
                            variant={statusVariant[job.status] || "outline"}
                          >
                            {isRunning && (
                              <span className="relative mr-1.5 flex h-2 w-2">
                                <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-current opacity-75" />
                                <span className="relative inline-flex h-2 w-2 rounded-full bg-current" />
                              </span>
                            )}
                            {job.status}
                          </Badge>
                        </TableCell>
                        <TableCell className="text-sm max-w-[200px] truncate hidden md:table-cell">
                          {job.index_uid}
                        </TableCell>
                        <TableCell>
                          {isRunning ? (
                            <div className="space-y-1">
                              {progressPercent !== null ? (
                                <div className="flex items-center gap-2">
                                  <Progress
                                    value={progressPercent}
                                    className="h-1.5 w-20"
                                  />
                                  <span className="text-xs text-muted-foreground">
                                    {job.pages_crawled}/{job.max_pages}
                                  </span>
                                </div>
                              ) : (
                                <span className="text-sm">
                                  {job.pages_crawled} pages
                                </span>
                              )}
                            </div>
                          ) : (
                            <div>
                              <span className="text-sm">
                                {job.pages_crawled} pages
                              </span>
                              {job.errors > 0 && (
                                <span className="text-xs text-destructive ml-2">
                                  {job.errors} err
                                </span>
                              )}
                            </div>
                          )}
                        </TableCell>
                        <TableCell className="text-sm text-muted-foreground hidden sm:table-cell">
                          {job.started_at ? (
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <span>
                                  {formatDistanceToNow(new Date(job.started_at), {
                                    addSuffix: true,
                                  })}
                                </span>
                              </TooltipTrigger>
                              <TooltipContent>
                                {new Date(job.started_at).toLocaleString()}
                              </TooltipContent>
                            </Tooltip>
                          ) : (
                            "—"
                          )}
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

              {/* Pagination */}
              {totalPages > 1 && (
                <div className="flex items-center justify-between pt-2">
                  <p className="text-xs text-muted-foreground">
                    Showing {safePage * PAGE_SIZE + 1}–
                    {Math.min((safePage + 1) * PAGE_SIZE, filteredJobs.length)}{" "}
                    of {filteredJobs.length}
                  </p>
                  <Pagination className="mx-0 w-auto">
                    <PaginationContent>
                      <PaginationItem>
                        <PaginationPrevious
                          onClick={() => setPage(safePage - 1)}
                          aria-disabled={safePage === 0}
                          className={cn(
                            safePage === 0 && "pointer-events-none opacity-50"
                          )}
                        />
                      </PaginationItem>
                      <PaginationItem>
                        <span className="text-sm text-muted-foreground px-2">
                          {safePage + 1} / {totalPages}
                        </span>
                      </PaginationItem>
                      <PaginationItem>
                        <PaginationNext
                          onClick={() => setPage(safePage + 1)}
                          aria-disabled={safePage >= totalPages - 1}
                          className={cn(
                            safePage >= totalPages - 1 &&
                              "pointer-events-none opacity-50"
                          )}
                        />
                      </PaginationItem>
                    </PaginationContent>
                  </Pagination>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>

      {/* Delete confirmation dialog */}
      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Job</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete{" "}
              <span className="font-medium">{deleteLabel}</span>? This action is
              permanent.
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
