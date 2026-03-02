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
import { Globe, HardDrive, Briefcase, Monitor } from "lucide-react";
import {
  fetchKpis,
  fetchHourlyStats,
  fetchTopDomains,
  fetchAccountUsage,
  fetchDailyUsage,
} from "@/lib/api";
import { getMe } from "@/lib/auth";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

const TIME_RANGES = [
  { label: "24h", hours: 24, days: 1 },
  { label: "7d", hours: 168, days: 7 },
  { label: "30d", hours: 720, days: 30 },
  { label: "90d", hours: 2160, days: 90 },
] as const;

function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0)} ${units[i]}`;
}

function formatNumber(n: number): string {
  return n.toLocaleString();
}

function formatHour(hour: string): string {
  try {
    const d = new Date(hour);
    return d.toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric" });
  } catch {
    return hour;
  }
}

export default function UsagePage() {
  const [rangeIdx, setRangeIdx] = useState(0);
  const range = TIME_RANGES[rangeIdx];

  const { data: user } = useQuery({
    queryKey: ["me"],
    queryFn: getMe,
    staleTime: 60_000,
  });

  const accountId = user?.account?.id;

  const { data: kpis, isLoading: kpisLoading } = useQuery({
    queryKey: ["analytics", "kpis", range.hours],
    queryFn: () => fetchKpis(range.hours),
    refetchInterval: 30_000,
  });

  const { data: hourly, isLoading: hourlyLoading } = useQuery({
    queryKey: ["analytics", "hourly", range.hours],
    queryFn: () => fetchHourlyStats(range.hours),
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

  const { data: dailyUsage, isLoading: dailyLoading } = useQuery({
    queryKey: ["analytics", "dailyUsage", accountId, range.days],
    queryFn: () => fetchDailyUsage(accountId!, range.days),
    enabled: !!accountId,
    refetchInterval: 30_000,
  });

  // Use account-specific data when available, fall back to global KPIs
  const usage = accountUsage?.data?.[0];
  const kpi = kpis?.data?.[0];

  const pagesCrawled = usage?.total_requests ?? kpi?.total_crawls ?? 0;
  const bandwidth = usage?.total_bytes ?? kpi?.total_bytes ?? 0;
  const jobs = usage?.total_jobs ?? 0;
  const jsRenders = usage?.js_renders ?? 0;

  const hourlyData = hourly?.data?.map((row) => ({
    ...row,
    hour: formatHour(row.hour),
  })) ?? [];

  const domainData = topDomains?.data?.slice(0, 10) ?? [];

  const dailyData = dailyUsage?.data ?? [];

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
                <CardTitle className="text-sm font-medium">Pages Crawled</CardTitle>
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
                <CardTitle className="text-sm font-medium">Jobs</CardTitle>
                <Briefcase className="h-4 w-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{formatNumber(jobs)}</div>
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

      {/* Crawl Activity Chart */}
      <Card>
        <CardHeader>
          <CardTitle>Crawl Activity</CardTitle>
          <CardDescription>Requests, successes, and failures over time</CardDescription>
        </CardHeader>
        <CardContent>
          {hourlyLoading ? (
            <Skeleton className="h-[300px] w-full" />
          ) : hourlyData.length > 0 ? (
            <ResponsiveContainer width="100%" height={300}>
              <AreaChart data={hourlyData}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                <XAxis
                  dataKey="hour"
                  tick={{ fontSize: 12 }}
                  className="text-muted-foreground"
                  interval="preserveStartEnd"
                />
                <YAxis tick={{ fontSize: 12 }} className="text-muted-foreground" />
                <Tooltip
                  contentStyle={{
                    backgroundColor: "hsl(var(--card))",
                    border: "1px solid hsl(var(--border))",
                    borderRadius: "var(--radius)",
                    fontSize: 12,
                  }}
                />
                <Area
                  type="monotone"
                  dataKey="successes"
                  stackId="1"
                  stroke="hsl(var(--chart-2))"
                  fill="hsl(var(--chart-2))"
                  fillOpacity={0.4}
                  name="Successes"
                />
                <Area
                  type="monotone"
                  dataKey="failures"
                  stackId="1"
                  stroke="hsl(var(--chart-5))"
                  fill="hsl(var(--chart-5))"
                  fillOpacity={0.4}
                  name="Failures"
                />
              </AreaChart>
            </ResponsiveContainer>
          ) : (
            <p className="text-sm text-muted-foreground py-12 text-center">
              No activity data for this period
            </p>
          )}
        </CardContent>
      </Card>

      {/* Bandwidth + Top Domains side-by-side */}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Bandwidth Over Time</CardTitle>
            <CardDescription>Data transferred per hour</CardDescription>
          </CardHeader>
          <CardContent>
            {hourlyLoading ? (
              <Skeleton className="h-[250px] w-full" />
            ) : hourlyData.length > 0 ? (
              <ResponsiveContainer width="100%" height={250}>
                <AreaChart data={hourlyData}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                  <XAxis
                    dataKey="hour"
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
                    contentStyle={{
                      backgroundColor: "hsl(var(--card))",
                      border: "1px solid hsl(var(--border))",
                      borderRadius: "var(--radius)",
                      fontSize: 12,
                    }}
                    formatter={(value: number) => [formatBytes(value), "Bandwidth"]}
                  />
                  <Area
                    type="monotone"
                    dataKey="total_bytes"
                    stroke="hsl(var(--chart-1))"
                    fill="hsl(var(--chart-1))"
                    fillOpacity={0.3}
                    name="Bandwidth"
                  />
                </AreaChart>
              </ResponsiveContainer>
            ) : (
              <p className="text-sm text-muted-foreground py-12 text-center">
                No bandwidth data for this period
              </p>
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
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "hsl(var(--card))",
                      border: "1px solid hsl(var(--border))",
                      borderRadius: "var(--radius)",
                      fontSize: 12,
                    }}
                  />
                  <Bar
                    dataKey="total_requests"
                    fill="hsl(var(--chart-3))"
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
            {dailyLoading ? (
              <div className="space-y-2">
                {Array.from({ length: 5 }).map((_, i) => (
                  <Skeleton key={i} className="h-8 w-full" />
                ))}
              </div>
            ) : dailyData.length > 0 ? (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Date</TableHead>
                    <TableHead className="text-right">Requests</TableHead>
                    <TableHead className="text-right">Bandwidth</TableHead>
                    <TableHead className="text-right">Jobs</TableHead>
                    <TableHead className="text-right">JS Renders</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {dailyData.map((row) => (
                    <TableRow key={row.date}>
                      <TableCell className="font-medium">{row.date}</TableCell>
                      <TableCell className="text-right">{formatNumber(row.requests)}</TableCell>
                      <TableCell className="text-right">{formatBytes(row.bytes)}</TableCell>
                      <TableCell className="text-right">{formatNumber(row.jobs)}</TableCell>
                      <TableCell className="text-right">{formatNumber(row.js_renders)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            ) : (
              <p className="text-sm text-muted-foreground py-8 text-center">
                No daily usage data for this period
              </p>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
