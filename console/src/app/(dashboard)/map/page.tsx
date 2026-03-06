"use client";

import { useState, useCallback } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
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
  Network,
  Search,
  ExternalLink,
  Clock,
  Link2,
  Loader2,
  Copy,
  Download,
} from "lucide-react";
import { submitMap } from "@/lib/api";
import type { MapResult, MapLink } from "@/lib/api-types";

export default function MapPage() {
  const [url, setUrl] = useState("https://example.com");
  const [depth, setDepth] = useState("2");
  const [limit, setLimit] = useState("5000");
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<MapResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [filterText, setFilterText] = useState("");

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
        depth: parseInt(depth) || 2,
        limit: parseInt(limit) || 5000,
        search: search.trim() || undefined,
      });
      setResult(data);
    } catch (err) {
      const msg =
        err instanceof Error
          ? err.message
          : "Failed to fetch. Is the API running?";
      setError(msg);
    }

    setLoading(false);
  }, [url, depth, limit, search]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") handleMap();
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
    const header = "url,title,description";
    const rows = filteredLinks.map(
      (l) =>
        `"${l.url}","${(l.title ?? "").replace(/"/g, '""')}","${(l.description ?? "").replace(/"/g, '""')}"`
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
    <div className="flex flex-col gap-4 h-[calc(100vh-6rem)]">
      {/* URL Input + Options */}
      <Card>
        <CardContent className="p-4">
          <div className="flex flex-col gap-4">
            <div className="flex gap-2">
              <div className="flex-1">
                <Input
                  placeholder="https://example.com"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  onKeyDown={handleKeyDown}
                  className="font-mono text-sm"
                />
              </div>
              <Button onClick={handleMap} disabled={loading}>
                {loading ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <Network className="mr-2 h-4 w-4" />
                )}
                Map
              </Button>
            </div>

            <div className="flex flex-wrap gap-4 items-end">
              <div className="space-y-1">
                <Label className="text-xs text-muted-foreground">Depth</Label>
                <Input
                  type="number"
                  min={0}
                  max={5}
                  value={depth}
                  onChange={(e) => setDepth(e.target.value)}
                  className="w-20 text-sm"
                />
              </div>
              <div className="space-y-1">
                <Label className="text-xs text-muted-foreground">Limit</Label>
                <Input
                  type="number"
                  min={1}
                  max={10000}
                  value={limit}
                  onChange={(e) => setLimit(e.target.value)}
                  className="w-24 text-sm"
                />
              </div>
              <div className="space-y-1 flex-1 min-w-[200px]">
                <Label className="text-xs text-muted-foreground">
                  Search filter (server-side)
                </Label>
                <Input
                  placeholder="Filter by keyword..."
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  onKeyDown={handleKeyDown}
                  className="text-sm"
                />
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Results */}
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

              {/* Client-side filter */}
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
                      <TableHead className="w-[40%]">URL</TableHead>
                      <TableHead className="w-[25%]">Title</TableHead>
                      <TableHead>Description</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {filteredLinks.map((link, i) => (
                      <MapRow key={i} link={link} />
                    ))}
                    {filteredLinks.length === 0 && (
                      <TableRow>
                        <TableCell
                          colSpan={3}
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
          <Card className="h-full flex items-center justify-center">
            <div className="text-center text-muted-foreground space-y-2">
              <Network className="h-10 w-10 mx-auto opacity-30" />
              <p className="text-sm">
                Enter a URL to discover all pages on a website.
              </p>
              <p className="text-xs">
                Uses sitemaps and BFS link crawling to find pages with their
                titles and descriptions.
              </p>
            </div>
          </Card>
        )}
      </div>
    </div>
  );
}

function MapRow({ link }: { link: MapLink }) {
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
      <TableCell className="text-sm max-w-0">
        <span className="truncate block" title={link.title}>
          {link.title ?? (
            <span className="text-muted-foreground italic">--</span>
          )}
        </span>
      </TableCell>
      <TableCell className="text-xs text-muted-foreground max-w-0">
        <span className="truncate block" title={link.description}>
          {link.description ?? (
            <span className="italic">--</span>
          )}
        </span>
      </TableCell>
    </TableRow>
  );
}
