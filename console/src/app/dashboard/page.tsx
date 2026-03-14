"use client";

import Link from "next/link";
import { useQuery } from "@tanstack/react-query";
import { useMe } from "@/lib/hooks";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { cn } from "@/lib/utils";
import {
  Globe,
  Zap,
  Database,
  TrendingUp,
  AlertCircle,
  ArrowRight,
  Play,
} from "lucide-react";
import { fetchStats, fetchErrors } from "@/lib/api";

function StatCard({
  name,
  value,
  icon: Icon,
  href,
}: {
  name: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
  href?: string;
}) {
  const content = (
    <Card className={cn("glitch-card glitch-glow-panel", href && "transition-colors hover:border-primary/50 cursor-pointer")}>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{name}</CardTitle>
        <Icon className="h-4 w-4 text-muted-foreground" />
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold glitch-metric">{value}</div>
      </CardContent>
    </Card>
  );
  if (href) return <Link href={href}>{content}</Link>;
  return content;
}

export default function DashboardPage() {
  const { data: user } = useMe();

  const {
    data: stats,
    isLoading: statsLoading,
    error: statsError,
  } = useQuery({
    queryKey: ["stats"],
    queryFn: fetchStats,
    refetchInterval: 10_000,
  });

  const { data: errors } = useQuery({
    queryKey: ["errors", 5],
    queryFn: () => fetchErrors(5),
    refetchInterval: 10_000,
  });

  const successRate =
    stats && stats.diagnostics.total_requests > 0
      ? (
          (stats.diagnostics.total_successes /
            stats.diagnostics.total_requests) *
          100
        ).toFixed(1)
      : "0.0";

  const statCards: {
    name: string;
    value: string;
    icon: React.ComponentType<{ className?: string }>;
    href?: string;
  }[] = stats
    ? [
        {
          name: "Total Requests",
          value: stats.diagnostics.total_requests.toLocaleString(),
          icon: Globe,
        },
        {
          name: "Active Jobs",
          value: stats.jobs.running.toLocaleString(),
          icon: Zap,
          href: "/dashboard/jobs",
        },
        {
          name: "Domains Tracked",
          value: stats.diagnostics.tracked_domains.toLocaleString(),
          icon: Database,
        },
        {
          name: "Success Rate",
          value: `${successRate}%`,
          icon: TrendingUp,
        },
      ]
    : [];

  const account = user?.account ?? null;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight glitch-brand">Dashboard</h2>
          <p className="text-muted-foreground">
            {account
              ? `Welcome back, ${account.name}`
              : "Your crawling overview"}
          </p>
        </div>
        <Button asChild className="glitch-btn-primary">
          <Link href="/dashboard/playground">
            <Play className="h-4 w-4 mr-2" />
            New Crawl
          </Link>
        </Button>
      </div>

      {statsError && (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>
            Could not reach the Scrapix API:{" "}
            {statsError instanceof Error
              ? statsError.message
              : "Unknown error"}
          </AlertDescription>
        </Alert>
      )}

      {/* Stats Grid */}
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
        {statsLoading
          ? Array.from({ length: 4 }).map((_, i) => (
              <Card key={i}>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <Skeleton className="h-4 w-24" />
                </CardHeader>
                <CardContent>
                  <Skeleton className="h-8 w-16" />
                </CardContent>
              </Card>
            ))
          : statCards.map((stat) => (
              <StatCard key={stat.name} {...stat} />
            ))}
      </div>

      <div className="grid gap-4 lg:grid-cols-3">
        {/* Recent Errors */}
        <Card className="lg:col-span-2 glitch-card glitch-glow-panel glitch-particles">
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <div>
                <CardTitle>Recent Errors</CardTitle>
                <CardDescription>
                  {errors
                    ? `${errors.total_count} total errors tracked`
                    : "Loading..."}
                </CardDescription>
              </div>
            </div>
          </CardHeader>
          <CardContent>
            {errors && errors.errors.length > 0 ? (
              <div className="space-y-3">
                {errors.errors.map((err, i) => (
                  <div
                    key={i}
                    className="flex items-start justify-between gap-4 text-sm"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="font-mono text-xs truncate">{err.url}</p>
                      <p className="text-muted-foreground text-xs">
                        {err.error}
                      </p>
                    </div>
                    <div className="flex items-center gap-2 shrink-0">
                      {err.status && (
                        <Badge variant="outline">{err.status}</Badge>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            ) : errors ? (
              <p className="text-sm text-muted-foreground py-4 text-center">
                No recent errors
              </p>
            ) : (
              <div className="space-y-3">
                {Array.from({ length: 3 }).map((_, i) => (
                  <Skeleton key={i} className="h-8 w-full" />
                ))}
              </div>
            )}
          </CardContent>
        </Card>

        {/* Quick Actions */}
        <Card className="glitch-card glitch-glow-panel">
          <CardHeader>
            <CardTitle>Quick Actions</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <Button asChild variant="outline" className="w-full justify-between">
              <Link href="/dashboard/playground">
                Start a Crawl
                <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
            <Button asChild variant="outline" className="w-full justify-between">
              <Link href="/dashboard/jobs">
                View Jobs
                <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
            <Button asChild variant="outline" className="w-full justify-between">
              <Link href="/dashboard/api-keys">
                Manage API Keys
                <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>

            {stats && (
              <div className="pt-3 border-t space-y-2">
                <div className="flex items-center justify-between text-sm">
                  <span className="text-muted-foreground">Credits</span>
                  <span className="font-medium">
                    {(account as Record<string, unknown>)?.credits_balance != null
                      ? Number((account as Record<string, unknown>).credits_balance).toLocaleString()
                      : "0"}
                  </span>
                </div>
                <div className="flex items-center justify-between text-sm">
                  <span className="text-muted-foreground">Total Jobs</span>
                  <span className="font-medium">
                    {stats.jobs.total.toLocaleString()}
                  </span>
                </div>
                <div className="flex items-center justify-between text-sm">
                  <span className="text-muted-foreground">Completed</span>
                  <span className="font-medium">
                    {stats.jobs.completed.toLocaleString()}
                  </span>
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
