"use client";

import { useEffect, useState, useCallback } from "react";
import { createClient } from "@/lib/supabase/client";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Globe, Zap, Database, TrendingUp, AlertCircle } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { fetchStats, fetchErrors } from "@/lib/api";
import type { SystemStats, RecentErrors } from "@/lib/api-types";

interface Account {
  id: string;
  name: string;
  tier: string;
}

export default function DashboardPage() {
  const [account, setAccount] = useState<Account | null>(null);
  const [stats, setStats] = useState<SystemStats | null>(null);
  const [errors, setErrors] = useState<RecentErrors | null>(null);
  const [loading, setLoading] = useState(true);
  const [apiError, setApiError] = useState<string | null>(null);
  const supabase = createClient();

  useEffect(() => {
    const fetchAccount = async () => {
      const {
        data: { user },
      } = await supabase.auth.getUser();
      if (!user) return;

      const { data: membership } = await supabase
        .from("account_members")
        .select("account_id")
        .eq("user_id", user.id)
        .single();

      if (membership) {
        const { data: accountData } = await supabase
          .from("accounts")
          .select("*")
          .eq("id", membership.account_id)
          .single();

        if (accountData) {
          setAccount(accountData);
        }
      }
    };

    fetchAccount();
  }, [supabase]);

  const refreshData = useCallback(async () => {
    try {
      const [statsData, errorsData] = await Promise.all([
        fetchStats(),
        fetchErrors(5),
      ]);
      setStats(statsData);
      setErrors(errorsData);
      setApiError(null);
    } catch (e) {
      setApiError(
        e instanceof Error ? e.message : "Failed to connect to API"
      );
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refreshData();
    const interval = setInterval(refreshData, 10_000);
    return () => clearInterval(interval);
  }, [refreshData]);

  const statCards = stats
    ? [
        {
          name: "Pages Crawled",
          value: stats.total_pages_crawled.toLocaleString(),
          icon: Globe,
        },
        {
          name: "Active Jobs",
          value: stats.active_jobs.toLocaleString(),
          icon: Zap,
        },
        {
          name: "Domains Tracked",
          value: stats.domains_tracked.toLocaleString(),
          icon: Database,
        },
        {
          name: "Success Rate",
          value: `${stats.success_rate.toFixed(1)}%`,
          icon: TrendingUp,
        },
      ]
    : [];

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Dashboard</h2>
        <p className="text-muted-foreground">
          Welcome back{account ? `, ${account.name}` : ""}! Here&apos;s your
          usage overview.
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

      {/* Stats Grid */}
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {loading
          ? Array.from({ length: 4 }).map((_, i) => (
              <Card key={i}>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <div className="h-4 w-24 bg-muted animate-pulse rounded" />
                </CardHeader>
                <CardContent>
                  <div className="h-8 w-16 bg-muted animate-pulse rounded" />
                </CardContent>
              </Card>
            ))
          : statCards.map((stat) => (
              <Card key={stat.name}>
                <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                  <CardTitle className="text-sm font-medium">
                    {stat.name}
                  </CardTitle>
                  <stat.icon className="h-4 w-4 text-muted-foreground" />
                </CardHeader>
                <CardContent>
                  <div className="text-2xl font-bold">{stat.value}</div>
                </CardContent>
              </Card>
            ))}
      </div>

      {/* Recent Errors */}
      <Card>
        <CardHeader>
          <CardTitle>Recent Errors</CardTitle>
          <CardDescription>
            {errors
              ? `${errors.total} total errors tracked`
              : "Loading error data..."}
          </CardDescription>
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
                    <p className="text-muted-foreground text-xs">{err.error}</p>
                  </div>
                  <div className="flex items-center gap-2 shrink-0">
                    {err.status && (
                      <Badge variant="outline">{err.status}</Badge>
                    )}
                    <span className="text-xs text-muted-foreground">
                      {err.domain}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          ) : errors ? (
            <p className="text-sm text-muted-foreground">
              No recent errors — looking good!
            </p>
          ) : (
            <div className="h-20 flex items-center justify-center">
              <div className="h-4 w-48 bg-muted animate-pulse rounded" />
            </div>
          )}
        </CardContent>
      </Card>

      {/* Quick Actions */}
      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Quick Start</CardTitle>
            <CardDescription>
              Get started with Scrapix in seconds
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-4">
              <div className="rounded-lg bg-muted p-4">
                <p className="text-sm font-medium mb-2">Scrape a URL</p>
                <code className="text-xs bg-background p-2 rounded block overflow-x-auto">
                  curl -X POST{" "}
                  {process.env.NEXT_PUBLIC_SCRAPIX_API_URL ||
                    "https://api.scrapix.io"}
                  /scrape \
                  <br />
                  &nbsp;&nbsp;-H &quot;X-API-Key: YOUR_API_KEY&quot; \
                  <br />
                  &nbsp;&nbsp;-H &quot;Content-Type: application/json&quot; \
                  <br />
                  &nbsp;&nbsp;-d &apos;
                  {`{"url": "https://example.com"}`}&apos;
                </code>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Account Status</CardTitle>
            <CardDescription>Your current plan and limits</CardDescription>
          </CardHeader>
          <CardContent>
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <span className="text-sm text-muted-foreground">Plan</span>
                <span className="text-sm font-medium capitalize">
                  {account?.tier || "Free"}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-muted-foreground">
                  Total Jobs
                </span>
                <span className="text-sm font-medium">
                  {stats?.total_jobs.toLocaleString() ?? "-"}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-muted-foreground">
                  Completed / Failed
                </span>
                <span className="text-sm font-medium">
                  {stats
                    ? `${stats.completed_jobs} / ${stats.failed_jobs}`
                    : "-"}
                </span>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
