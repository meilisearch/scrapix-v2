"use client";

import { useState, useCallback, useEffect } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { toast } from "sonner";
import {
  Search,
  ExternalLink,
  Clock,
  Link2,
  Loader2,
  Copy,
  Download,
} from "lucide-react";
import { submitMap } from "@/lib/api";
import { CodeBlock } from "@/app/dashboard/playground/result-panel";
import { UrlBar } from "@/app/dashboard/playground/url-bar";
import { HistoryPanel, loadRuns, saveRun, type RunEntry } from "@/app/dashboard/playground/recent-runs";
import type { MapResult, MapLink } from "@/lib/api-types";

const MAP_EXAMPLE = `curl -X POST https://scrapix.meilisearch.dev/map \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer YOUR_API_KEY" \\
  -d '{
    "url": "https://example.com",
    "depth": 0,
    "limit": 5000,
    "get_title": true,
    "get_description": true,
    "get_lastmod": true,
    "get_priority": true,
    "get_changefreq": true
  }'`;

function SwitchRow({
  id,
  label,
  checked,
  onCheckedChange,
}: {
  id: string;
  label: string;
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between">
      <Label htmlFor={id} className="text-sm font-medium cursor-pointer">
        {label}
      </Label>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}

export default function MapPage() {
  const [url, setUrl] = useState("https://scrapix.meilisearch.dev");
  const [depth, setDepth] = useState("0");
  const [limit, setLimit] = useState("5000");
  const [search, setSearch] = useState("");
  const [getTitle, setGetTitle] = useState(true);
  const [getDescription, setGetDescription] = useState(true);
  const [getLastmod, setGetLastmod] = useState(true);
  const [getPriority, setGetPriority] = useState(true);
  const [getChangefreq, setGetChangefreq] = useState(true);
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<MapResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filterText, setFilterText] = useState("");
  const [runs, setRuns] = useState<RunEntry[]>([]);

  useEffect(() => {
    setRuns(loadRuns());
  }, []);

  const handleMap = useCallback(async () => {
    if (!url.trim()) {
      toast.error("Please enter a URL");
      return;
    }

    setLoading(true);
    setResult(null);
    setError(null);

    try {
      const data = await submitMap({
        url,
        depth: parseInt(depth) || 0,
        limit: parseInt(limit) || 5000,
        search: search.trim() || undefined,
        get_title: getTitle,
        get_description: getDescription,
        get_lastmod: getLastmod,
        get_priority: getPriority,
        get_changefreq: getChangefreq,
      });
      setResult(data);
      const newRuns = saveRun({
        id: Math.random().toString(36).slice(2) + Date.now().toString(36),
        type: "map",
        url,
        duration_ms: data.duration_ms,
        total_links: data.total,
        timestamp: new Date().toISOString(),
      });
      setRuns(newRuns);
    } catch (err) {
      const msg =
        err instanceof Error
          ? err.message
          : "Failed to fetch. Is the API running?";
      setError(msg);
    }

    setLoading(false);
  }, [url, depth, limit, search, getTitle, getDescription, getLastmod, getPriority, getChangefreq]);

  const handleReplay = (run: RunEntry) => {
    setUrl(run.url);
  };

  const filteredLinks =
    result?.links.filter((link) => {
      if (!filterText.trim()) return true;
      const q = filterText.toLowerCase();
      return (
        link.url.toLowerCase().includes(q) ||
        link.title?.toLowerCase().includes(q) ||
        link.description?.toLowerCase().includes(q)
      );
    }) ?? [];

  const copyAllUrls = () => {
    const urls = filteredLinks.map((l) => l.url).join("\n");
    navigator.clipboard.writeText(urls);
    toast.success(`Copied ${filteredLinks.length} URLs`);
  };

  const exportCsv = () => {
    const header = "url,title,description,lastmod,priority,changefreq";
    const rows = filteredLinks.map(
      (l) =>
        `"${l.url}","${(l.title ?? "").replace(/"/g, '""')}","${(l.description ?? "").replace(/"/g, '""')}","${l.lastmod ?? ""}","${l.priority ?? ""}","${l.changefreq ?? ""}"`
    );
    const csv = [header, ...rows].join("\n");
    const blob = new Blob([csv], { type: "text/csv" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `map-${new URL(url).hostname}-${Date.now()}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  return (
    <div className="flex flex-col gap-4 h-full">
      <UrlBar
        mode="map"
        url={url}
        onUrlChange={setUrl}
        onSubmit={handleMap}
        loading={loading}
        historySlot={
          <div className="p-3 h-full">
            <HistoryPanel runs={runs} onReplay={handleReplay} typeFilter="map" />
          </div>
        }
      />

      <div className="grid grid-cols-1 lg:grid-cols-[minmax(280px,1fr)_3fr] gap-4 flex-1 min-h-0">
        <Card className="overflow-auto">
          <CardContent className="p-4">
            <div className="space-y-5">
              <div className="space-y-3">
                <Label className="text-xs text-muted-foreground uppercase tracking-wide">
                  Parameters
                </Label>
                <div className="grid grid-cols-2 gap-3">
                  <div className="space-y-1.5">
                    <Label htmlFor="depth" className="text-sm font-medium">Depth</Label>
                    <Input
                      id="depth"
                      type="number"
                      min={0}
                      max={5}
                      value={depth}
                      onChange={(e) => setDepth(e.target.value)}
                      className="text-sm"
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label htmlFor="limit" className="text-sm font-medium">Limit</Label>
                    <Input
                      id="limit"
                      type="number"
                      min={1}
                      max={10000}
                      value={limit}
                      onChange={(e) => setLimit(e.target.value)}
                      className="text-sm"
                    />
                  </div>
                </div>
                <div className="space-y-1.5">
                  <Label htmlFor="search-filter" className="text-sm font-medium">
                    Search filter
                  </Label>
                  <p className="text-xs text-muted-foreground">Server-side keyword filter</p>
                  <Input
                    id="search-filter"
                    placeholder="Filter by keyword..."
                    value={search}
                    onChange={(e) => setSearch(e.target.value)}
                    className="text-sm"
                  />
                </div>
              </div>

              <div className="space-y-3 border-t pt-4">
                <Label className="text-xs text-muted-foreground uppercase tracking-wide">
                  Include Fields
                </Label>
                <SwitchRow id="get-title" label="Title" checked={getTitle} onCheckedChange={setGetTitle} />
                <SwitchRow id="get-description" label="Description" checked={getDescription} onCheckedChange={setGetDescription} />
                <SwitchRow id="get-lastmod" label="Last Modified" checked={getLastmod} onCheckedChange={setGetLastmod} />
                <SwitchRow id="get-priority" label="Priority" checked={getPriority} onCheckedChange={setGetPriority} />
                <SwitchRow id="get-changefreq" label="Change Freq" checked={getChangefreq} onCheckedChange={setGetChangefreq} />
              </div>
            </div>
          </CardContent>
        </Card>

        <div className="flex-1 min-h-0">
          {loading && (
            <Card className="h-full">
              <CardContent className="p-6 space-y-4">
                <div className="flex items-center gap-2 text-muted-foreground">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  <span className="text-sm">
                    Mapping website... This may take a moment for deep crawls.
                  </span>
                </div>
                {Array.from({ length: 8 }).map((_, i) => (
                  <div key={i} className="flex gap-4 items-center">
                    <Skeleton className="h-4 flex-[3]" />
                    <Skeleton className="h-4 flex-[2]" />
                    <Skeleton className="h-4 flex-[4]" />
                  </div>
                ))}
              </CardContent>
            </Card>
          )}

          {error && (
            <Card className="border-destructive">
              <CardContent className="p-6">
                <p className="text-destructive text-sm">{error}</p>
              </CardContent>
            </Card>
          )}

          {result && (
            <Card className="h-full flex flex-col">
              <CardHeader className="pb-3 flex-none">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <CardTitle className="text-base">
                      Discovered Links
                    </CardTitle>
                    <Badge variant="secondary" className="font-mono">
                      <Link2 className="mr-1 h-3 w-3" />
                      {result.total}
                    </Badge>
                    <Badge variant="outline" className="text-xs font-normal">
                      <Clock className="mr-1 h-3 w-3" />
                      {result.duration_ms < 1000
                        ? `${result.duration_ms}ms`
                        : `${(result.duration_ms / 1000).toFixed(1)}s`}
                    </Badge>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={copyAllUrls}
                    >
                      <Copy className="mr-1.5 h-3 w-3" />
                      Copy URLs
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={exportCsv}
                    >
                      <Download className="mr-1.5 h-3 w-3" />
                      CSV
                    </Button>
                  </div>
                </div>

                <div className="mt-3">
                  <div className="relative">
                    <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
                    <Input
                      placeholder="Filter results..."
                      value={filterText}
                      onChange={(e) => setFilterText(e.target.value)}
                      className="pl-9 text-sm"
                    />
                  </div>
                  {filterText && (
                    <p className="text-xs text-muted-foreground mt-1.5">
                      Showing {filteredLinks.length} of {result.links.length}
                    </p>
                  )}
                </div>
              </CardHeader>

              <CardContent className="flex-1 min-h-0 p-0">
                <ScrollArea className="h-full">
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="min-w-[250px]">URL</TableHead>
                        {getTitle && <TableHead>Title</TableHead>}
                        {getDescription && <TableHead>Description</TableHead>}
                        {getLastmod && <TableHead className="w-[120px]">Last Modified</TableHead>}
                        {getPriority && <TableHead className="w-[80px]">Priority</TableHead>}
                        {getChangefreq && <TableHead className="w-[100px]">Freq</TableHead>}
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {filteredLinks.map((link, i) => (
                        <MapRow
                          key={i}
                          link={link}
                          showTitle={getTitle}
                          showDescription={getDescription}
                          showLastmod={getLastmod}
                          showPriority={getPriority}
                          showChangefreq={getChangefreq}
                        />
                      ))}
                      {filteredLinks.length === 0 && (
                        <TableRow>
                          <TableCell
                            colSpan={1 + (getTitle ? 1 : 0) + (getDescription ? 1 : 0) + (getLastmod ? 1 : 0) + (getPriority ? 1 : 0) + (getChangefreq ? 1 : 0)}
                            className="text-center text-muted-foreground py-8"
                          >
                            No links match your filter.
                          </TableCell>
                        </TableRow>
                      )}
                    </TableBody>
                  </Table>
                </ScrollArea>
              </CardContent>
            </Card>
          )}

          {!loading && !error && !result && (
            <Card className="h-full flex flex-col">
              <div className="px-6 pt-4 pb-2">
                <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                  API Example
                </p>
              </div>
              <div className="px-6 pb-6 flex-1 min-h-0">
                <CodeBlock code={MAP_EXAMPLE} lang="bash" />
              </div>
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}

function MapRow({
  link,
  showTitle,
  showDescription,
  showLastmod,
  showPriority,
  showChangefreq,
}: {
  link: MapLink;
  showTitle: boolean;
  showDescription: boolean;
  showLastmod: boolean;
  showPriority: boolean;
  showChangefreq: boolean;
}) {
  const dash = <span className="text-muted-foreground italic">--</span>;

  return (
    <TableRow className="group">
      <TableCell className="font-mono text-xs max-w-0">
        <div className="flex items-center gap-1.5">
          <a
            href={link.url}
            target="_blank"
            rel="noopener noreferrer"
            className="truncate text-primary hover:underline"
            title={link.url}
          >
            {link.url}
          </a>
          <a
            href={link.url}
            target="_blank"
            rel="noopener noreferrer"
            className="opacity-0 group-hover:opacity-100 shrink-0"
          >
            <ExternalLink className="h-3 w-3 text-muted-foreground" />
          </a>
        </div>
      </TableCell>
      {showTitle && (
        <TableCell className="text-sm max-w-0">
          <span className="truncate block" title={link.title}>
            {link.title ?? dash}
          </span>
        </TableCell>
      )}
      {showDescription && (
        <TableCell className="text-xs text-muted-foreground max-w-0">
          <span className="truncate block" title={link.description}>
            {link.description ?? dash}
          </span>
        </TableCell>
      )}
      {showLastmod && (
        <TableCell className="text-xs text-muted-foreground">
          {link.lastmod ? new Date(link.lastmod).toLocaleDateString() : dash}
        </TableCell>
      )}
      {showPriority && (
        <TableCell className="text-xs text-muted-foreground text-center">
          {link.priority != null ? link.priority.toFixed(1) : dash}
        </TableCell>
      )}
      {showChangefreq && (
        <TableCell className="text-xs text-muted-foreground">
          {link.changefreq ?? dash}
        </TableCell>
      )}
    </TableRow>
  );
}
