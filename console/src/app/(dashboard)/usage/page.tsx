"use client";

import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Globe, HardDrive, Sparkles, Monitor } from "lucide-react";
import {
  fetchKpis,
  fetchHourlyStats,
  fetchDailyStats,
  fetchTopDomains,
  fetchAccountUsage,
  fetchDailyUsage,
} from "@/lib/api";
import { useMe } from "@/lib/hooks";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const TIME_RANGES = [
  { label: "24h", hours: 24, days: 1, granularity: "hourly" as const },
  { label: "7d", hours: 168, days: 7, granularity: "hourly" as const },
  { label: "30d", hours: 720, days: 30, granularity: "daily" as const },
  { label: "90d", hours: 2160, days: 90, granularity: "daily" as const },
];

// ---------------------------------------------------------------------------
// Formatters
// ---------------------------------------------------------------------------

function formatBytes(bytes: number | undefined | null): string {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

function formatNumber(n: number | undefined | null): string {
  return (n ?? 0).toLocaleString();
}

function formatHour(hour: string): string {
  try {
    const d = new Date(hour);
    return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric" });
  } catch {
    return hour;
  }
}

function formatDate(date: string): string {
  try {
    const d = new Date(date + "T00:00:00");
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  } catch {
    return date;
  }
}

// ---------------------------------------------------------------------------
// Gap-filling helpers
// ---------------------------------------------------------------------------

interface ChartPoint {
  label: string;
  requests: number;
  successes: number;
  failures: number;
  total_bytes: number;
}

function fillHourlyGaps(
  data: { hour: string; requests: number; successes: number; failures: number; total_bytes: number }[],
  hours: number,
): ChartPoint[] {
  const now = new Date();
  now.setMinutes(0, 0, 0);
  const byKey = new Map(data.map((d) => {
    const k = new Date(d.hour);
    k.setMinutes(0, 0, 0);
    return [k.getTime(), d];
  }));

  const result: ChartPoint[] = [];
  for (let i = hours - 1; i >= 0; i--) {
    const t = new Date(now.getTime() - i * 3600_000);
    const existing = byKey.get(t.getTime());
    result.push({
      label: formatHour(t.toISOString()),
      requests: existing?.requests ?? 0,
      successes: existing?.successes ?? 0,
      failures: existing?.failures ?? 0,
      total_bytes: existing?.total_bytes ?? 0,
    });
  }
  return result;
}

function fillDailyChartGaps(
  data: { date: string; requests: number; successes: number; failures: number; total_bytes: number }[],
  days: number,
): ChartPoint[] {
  const now = new Date();
  now.setHours(0, 0, 0, 0);
  const byKey = new Map(data.map((d) => [d.date, d]));

  const result: ChartPoint[] = [];
  for (let i = days - 1; i >= 0; i--) {
    const t = new Date(now.getTime() - i * 86400_000);
    const key = t.toISOString().slice(0, 10);
    const existing = byKey.get(key);
    result.push({
      label: formatDate(key),
      requests: existing?.requests ?? 0,
      successes: existing?.successes ?? 0,
      failures: existing?.failures ?? 0,
      total_bytes: existing?.total_bytes ?? 0,
    });
  }
  return result;
}

/** Fill missing days for billing table. */
function fillBillingDailyGaps(
  data: { date: string; requests: number; bytes: number; js_renders: number; ai_prompt_tokens: number; ai_completion_tokens: number }[],
  days: number,
) {
  const now = new Date();
  now.setHours(0, 0, 0, 0);
  const byKey = new Map(data.map((d) => [d.date, d]));

  const result: typeof data = [];
  for (let i = days - 1; i >= 0; i--) {
    const t = new Date(now.getTime() - i * 86400_000);
    const key = t.toISOString().slice(0, 10);
    result.push(byKey.get(key) ?? {
      date: key,
      requests: 0,
      bytes: 0,
      js_renders: 0,
      ai_prompt_tokens: 0,
      ai_completion_tokens: 0,
    });
  }
  return result;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export default function UsagePage() {
  const [rangeIdx, setRangeIdx] = useState(0);
  const range = TIME_RANGES[rangeIdx];
  const useDaily = range.granularity === "daily";

  const { data: user } = useMe();
  const accountId = user?.account?.id;

  // --- Queries ---

  const { data: kpis, isLoading: kpisLoading } = useQuery({
    queryKey: ["analytics", "kpis", range.hours],
    queryFn: () => fetchKpis(range.hours),
    refetchInterval: 30_000,
  });

  const { data: hourly, isLoading: hourlyLoading } = useQuery({
    queryKey: ["analytics", "hourly", range.hours],
    queryFn: () => fetchHourlyStats(range.hours),
    enabled: !useDaily,
    refetchInterval: 30_000,
  });

  const { data: daily, isLoading: dailyLoading } = useQuery({
    queryKey: ["analytics", "daily", range.days],
    queryFn: () => fetchDailyStats(range.days),
    enabled: useDaily,
    refetchInterval: 30_000,
  });

  const { data: topDomains } = useQuery({
    queryKey: ["analytics", "topDomains", range.hours],
    queryFn: () => fetchTopDomains(range.hours, 10),
    refetchInterval: 30_000,
  });

  const { data: accountUsage } = useQuery({
    queryKey: ["analytics", "accountUsage", accountId, range.hours],
    queryFn: () => fetchAccountUsage(accountId!, range.hours),
    enabled: !!accountId,
    refetchInterval: 30_000,
  });

  const { data: billingDaily, isLoading: billingDailyLoading } = useQuery({
    queryKey: ["analytics", "billingDaily", accountId, range.days],
    queryFn: () => fetchDailyUsage(accountId!, range.days),
    enabled: !!accountId,
    refetchInterval: 30_000,
  });

  // --- Derived data ---

  const usage = accountUsage?.data?.[0];
  const kpi = kpis?.data?.[0];

  const pagesCrawled = usage?.total_requests ?? kpi?.total_crawls ?? 0;
  const bandwidth = usage?.total_bytes ?? kpi?.total_bytes ?? 0;
  const aiTokens = (usage?.ai_prompt_tokens ?? 0) + (usage?.ai_completion_tokens ?? 0);
  const jsRenders = usage?.js_renders ?? 0;

  const chartLoading = useDaily ? dailyLoading : hourlyLoading;
  const chartData: ChartPoint[] = useDaily
    ? fillDailyChartGaps(daily?.data ?? [], range.days)
    : fillHourlyGaps(hourly?.data ?? [], range.hours);

  const domainData = topDomains?.data?.slice(0, 10) ?? [];
  const billingData = fillBillingDailyGaps(billingDaily?.data ?? [], range.days);

  const chartSubtitle = useDaily
    ? "Requests, successes, and failures per day"
    : "Requests, successes, and failures per hour";
  const bwSubtitle = useDaily ? "Data transferred per day" : "Data transferred per hour";

  // --- Chart colors ---
  const colors = {
    success: "#22c55e",    // green-500
    failure: "#ef4444",    // red-500
    bandwidth: "#3b82f6",  // blue-500
    domains: "#8b5cf6",    // violet-500
  };

  // --- Shared tooltip style ---
  const tooltipStyle = {
    backgroundColor: "hsl(var(--card))",
    border: "1px solid hsl(var(--border))",
    borderRadius: "var(--radius)",
    fontSize: 12,
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Usage</h2>
          <p className="text-muted-foreground">
            Monitor crawling activity, bandwidth, and billing metrics
          </p>
        </div>
        <ToggleGroup
          type="single"
          value={range.label}
          onValueChange={(v) => {
            const idx = TIME_RANGES.findIndex((r) => r.label === v);
            if (idx >= 0) setRangeIdx(idx);
          }}
        >
          {TIME_RANGES.map((r) => (
            <ToggleGroupItem key={r.label} value={r.label} size="sm">
              {r.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      {/* KPI Cards */}
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
        {kpisLoading ? (
          Array.from({ length: 4 }).map((_, i) => (
            <Card key={i}>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <Skeleton className="h-4 w-24" />
              </CardHeader>
              <CardContent>
                <Skeleton className="h-8 w-20" />
              </CardContent>
            </Card>
          ))
        ) : (
          <>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Total Requests</CardTitle>
                <Globe className="h-4 w-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{formatNumber(pagesCrawled)}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">Bandwidth</CardTitle>
                <HardDrive className="h-4 w-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{formatBytes(bandwidth)}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">AI Tokens</CardTitle>
                <Sparkles className="h-4 w-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{formatNumber(aiTokens)}</div>
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="flex flex-row items-center justify-between space-y-0 pb-2">
                <CardTitle className="text-sm font-medium">JS Renders</CardTitle>
                <Monitor className="h-4 w-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{formatNumber(jsRenders)}</div>
              </CardContent>
            </Card>
          </>
        )}
      </div>

      {/* Activity Chart */}
      <Card>
        <CardHeader>
          <CardTitle>Activity</CardTitle>
          <CardDescription>{chartSubtitle}</CardDescription>
        </CardHeader>
        <CardContent>
          {chartLoading ? (
            <Skeleton className="h-[300px] w-full" />
          ) : (
            <ResponsiveContainer width="100%" height={300}>
              <AreaChart data={chartData}>
                <defs>
                  <linearGradient id="gradSuccess" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor={colors.success} stopOpacity={0.4} />
                    <stop offset="95%" stopColor={colors.success} stopOpacity={0.05} />
                  </linearGradient>
                  <linearGradient id="gradFailure" x1="0" y1="0" x2="0" y2="1">
                    <stop offset="5%" stopColor={colors.failure} stopOpacity={0.4} />
                    <stop offset="95%" stopColor={colors.failure} stopOpacity={0.05} />
                  </linearGradient>
                </defs>
                <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                <XAxis
                  dataKey="label"
                  tick={{ fontSize: 12 }}
                  className="text-muted-foreground"
                  interval="preserveStartEnd"
                />
                <YAxis tick={{ fontSize: 12 }} className="text-muted-foreground" />
                <Tooltip contentStyle={tooltipStyle} />
                <Legend />
                <Area
                  type="monotone"
                  dataKey="successes"
                  stackId="1"
                  stroke={colors.success}
                  fill="url(#gradSuccess)"
                  strokeWidth={2}
                  name="Successes"
                />
                <Area
                  type="monotone"
                  dataKey="failures"
                  stackId="1"
                  stroke={colors.failure}
                  fill="url(#gradFailure)"
                  strokeWidth={2}
                  name="Failures"
                />
              </AreaChart>
            </ResponsiveContainer>
          )}
        </CardContent>
      </Card>

      {/* Bandwidth + Top Domains */}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Bandwidth Over Time</CardTitle>
            <CardDescription>{bwSubtitle}</CardDescription>
          </CardHeader>
          <CardContent>
            {chartLoading ? (
              <Skeleton className="h-[250px] w-full" />
            ) : (
              <ResponsiveContainer width="100%" height={250}>
                <AreaChart data={chartData}>
                  <defs>
                    <linearGradient id="gradBandwidth" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%" stopColor={colors.bandwidth} stopOpacity={0.4} />
                      <stop offset="95%" stopColor={colors.bandwidth} stopOpacity={0.05} />
                    </linearGradient>
                  </defs>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                  <XAxis
                    dataKey="label"
                    tick={{ fontSize: 12 }}
                    className="text-muted-foreground"
                    interval="preserveStartEnd"
                  />
                  <YAxis
                    tick={{ fontSize: 12 }}
                    className="text-muted-foreground"
                    tickFormatter={(v: number) => formatBytes(v)}
                  />
                  <Tooltip
                    contentStyle={tooltipStyle}
                    formatter={(value: number) => [formatBytes(value), "Bandwidth"]}
                  />
                  <Area
                    type="monotone"
                    dataKey="total_bytes"
                    stroke={colors.bandwidth}
                    fill="url(#gradBandwidth)"
                    strokeWidth={2}
                    name="Bandwidth"
                  />
                </AreaChart>
              </ResponsiveContainer>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Top Domains</CardTitle>
            <CardDescription>Most crawled domains by request count</CardDescription>
          </CardHeader>
          <CardContent>
            {domainData.length > 0 ? (
              <ResponsiveContainer width="100%" height={250}>
                <BarChart data={domainData} layout="vertical">
                  <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                  <XAxis type="number" tick={{ fontSize: 12 }} className="text-muted-foreground" />
                  <YAxis
                    type="category"
                    dataKey="domain"
                    tick={{ fontSize: 11 }}
                    className="text-muted-foreground"
                    width={120}
                  />
                  <Tooltip contentStyle={tooltipStyle} />
                  <Bar
                    dataKey="total_requests"
                    fill={colors.domains}
                    radius={[0, 4, 4, 0]}
                    name="Requests"
                  />
                </BarChart>
              </ResponsiveContainer>
            ) : (
              <p className="text-sm text-muted-foreground py-12 text-center">
                No domain data for this period
              </p>
            )}
          </CardContent>
        </Card>
      </div>

      {/* Daily Breakdown Table */}
      {accountId && (
        <Card>
          <CardHeader>
            <CardTitle>Daily Breakdown</CardTitle>
            <CardDescription>Per-day usage for billing</CardDescription>
          </CardHeader>
          <CardContent>
            {billingDailyLoading ? (
              <div className="space-y-2">
                {Array.from({ length: 5 }).map((_, i) => (
                  <Skeleton key={i} className="h-8 w-full" />
                ))}
              </div>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Date</TableHead>
                    <TableHead className="text-right">Requests</TableHead>
                    <TableHead className="text-right">Bandwidth</TableHead>
                    <TableHead className="text-right">JS Renders</TableHead>
                    <TableHead className="text-right">AI Tokens</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {billingData.map((row) => (
                    <TableRow key={row.date}>
                      <TableCell className="font-medium">{row.date}</TableCell>
                      <TableCell className="text-right">{formatNumber(row.requests)}</TableCell>
                      <TableCell className="text-right">{formatBytes(row.bytes)}</TableCell>
                      <TableCell className="text-right">{formatNumber(row.js_renders)}</TableCell>
                      <TableCell className="text-right">{formatNumber((row.ai_prompt_tokens ?? 0) + (row.ai_completion_tokens ?? 0))}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
