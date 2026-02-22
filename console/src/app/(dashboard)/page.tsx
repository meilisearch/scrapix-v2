"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useQuery } from "@tanstack/react-query";
import { getMe, type AuthUser } from "@/lib/auth";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
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
import type { SystemStats } from "@/lib/api-types";

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
    <Card className={href ? "transition-colors hover:border-primary/50 cursor-pointer" : ""}>
      <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
        <CardTitle className="text-sm font-medium">{name}</CardTitle>
        <Icon className="h-4 w-4 text-muted-foreground" />
      </CardHeader>
      <CardContent>
        <div className="text-2xl font-bold">{value}</div>
      </CardContent>
    </Card>
  );
  if (href) return <Link href={href}>{content}</Link>;
  return content;
}

export default function DashboardPage() {
  const [user, setUser] = useState<AuthUser | null>(null);

  useEffect(() => {
    getMe().then(setUser).catch(() => {});
  }, []);

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
          href: "/jobs",
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

  const account = user?.account;

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Dashboard</h2>
          <p className="text-muted-foreground">
            {account
              ? `Welcome back, ${account.name}`
              : "Your crawling overview"}
          </p>
        </div>
        <Button asChild>
          <Link href="/playground">
            <Play className="h-4 w-4 mr-2" />
            New Crawl
          </Link>
        </Button>
      </div>

      {statsError && (
        <Card className="border-destructive">
          <CardContent className="py-4 flex items-center gap-3">
            <AlertCircle className="h-5 w-5 text-destructive" />
            <p className="text-sm text-destructive">
              Could not reach the Scrapix API:{" "}
              {statsError instanceof Error
                ? statsError.message
                : "Unknown error"}
            </p>
          </CardContent>
        </Card>
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
        <Card className="lg:col-span-2">
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
        <Card>
          <CardHeader>
            <CardTitle>Quick Actions</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            <Button asChild variant="outline" className="w-full justify-between">
              <Link href="/playground">
                Start a Crawl
                <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
            <Button asChild variant="outline" className="w-full justify-between">
              <Link href="/jobs">
                View Jobs
                <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>
            <Button asChild variant="outline" className="w-full justify-between">
              <Link href="/api-keys">
                Manage API Keys
                <ArrowRight className="h-4 w-4" />
              </Link>
            </Button>

            {stats && (
              <div className="pt-3 border-t space-y-2">
                <div className="flex items-center justify-between text-sm">
                  <span className="text-muted-foreground">Plan</span>
                  <span className="font-medium capitalize">
                    {account?.tier || "Free"}
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
