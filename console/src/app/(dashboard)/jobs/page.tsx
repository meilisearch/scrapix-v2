"use client";

import { useEffect, useState, useCallback } from "react";
import Link from "next/link";
import {
  Card,
  CardContent,
  CardDescription,
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Trash2, ExternalLink, AlertCircle } from "lucide-react";
import { fetchJobs, deleteJob } from "@/lib/api";
import type { Job } from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";

const statusVariant: Record<string, "default" | "secondary" | "destructive" | "outline"> = {
  completed: "default",
  running: "secondary",
  pending: "outline",
  failed: "destructive",
  cancelled: "destructive",
};

export default function JobsPage() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(true);
  const [apiError, setApiError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const data = await fetchJobs();
      setJobs(data);
      setApiError(null);
    } catch (e) {
      setApiError(e instanceof Error ? e.message : "Failed to fetch jobs");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 5_000);
    return () => clearInterval(interval);
  }, [refresh]);

  const handleDelete = async (id: string) => {
    if (!confirm("Are you sure you want to delete this job?")) return;
    try {
      await deleteJob(id);
      setJobs((prev) => prev.filter((j) => j.id !== id));
      toast.success("Job deleted");
    } catch {
      toast.error("Failed to delete job");
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

      {apiError && (
        <Card className="border-destructive">
          <CardContent className="py-4 flex items-center gap-3">
            <AlertCircle className="h-5 w-5 text-destructive" />
            <p className="text-sm text-destructive">
              Could not reach the Scrapix API: {apiError}
            </p>
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader>
          <CardTitle>Crawl Jobs</CardTitle>
          <CardDescription>
            {loading
              ? "Loading..."
              : `${jobs.length} job${jobs.length !== 1 ? "s" : ""}`}
          </CardDescription>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="space-y-3">
              {Array.from({ length: 3 }).map((_, i) => (
                <div
                  key={i}
                  className="h-12 bg-muted animate-pulse rounded"
                />
              ))}
            </div>
          ) : jobs.length === 0 ? (
            <p className="text-sm text-muted-foreground py-8 text-center">
              No jobs yet. Start a crawl from the{" "}
              <Link href="/playground" className="text-primary hover:underline">
                Playground
              </Link>
              .
            </p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Job ID</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Start URLs</TableHead>
                  <TableHead className="text-right">Pages</TableHead>
                  <TableHead>Created</TableHead>
                  <TableHead className="w-[100px]">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {jobs.map((job) => (
                  <TableRow key={job.id}>
                    <TableCell className="font-mono text-xs">
                      <Link
                        href={`/jobs/${job.id}`}
                        className="hover:underline text-primary"
                      >
                        {job.id.slice(0, 8)}...
                      </Link>
                    </TableCell>
                    <TableCell>
                      <Badge variant={statusVariant[job.status] || "outline"}>
                        {job.status}
                      </Badge>
                    </TableCell>
                    <TableCell className="max-w-[250px] truncate text-sm">
                      {job.config.start_urls.join(", ")}
                    </TableCell>
                    <TableCell className="text-right text-sm">
                      {job.pages_crawled}
                      {job.pages_failed > 0 && (
                        <span className="text-destructive ml-1">
                          ({job.pages_failed} failed)
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground">
                      {formatDistanceToNow(new Date(job.created_at), {
                        addSuffix: true,
                      })}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1">
                        <Button
                          variant="ghost"
                          size="icon"
                          asChild
                        >
                          <Link href={`/jobs/${job.id}`}>
                            <ExternalLink className="h-4 w-4" />
                          </Link>
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => handleDelete(job.id)}
                        >
                          <Trash2 className="h-4 w-4 text-destructive" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
