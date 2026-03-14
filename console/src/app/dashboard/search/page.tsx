"use client";

import { useState, useCallback, useEffect } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Search,
  ExternalLink,
  Clock,
  Hash,
  Loader2,
  Layers,
  AlertCircle,
} from "lucide-react";
import { toast } from "sonner";
import { submitSearch, createCrawl } from "@/lib/api";
import type { MeilisearchSearchResponse, MeilisearchHit } from "@/lib/api-types";
import { UrlBar } from "@/app/dashboard/playground/url-bar";
import { HistoryPanel, loadRuns, saveRun, type RunEntry } from "@/app/dashboard/playground/recent-runs";
import { CodeBlock } from "@/app/dashboard/playground/result-panel";
import { urlToIndexUid } from "@/lib/crawl-config-utils";
import { useRouter } from "next/navigation";

const SEARCH_EXAMPLE = `curl -X POST https://scrapix.meilisearch.dev/search \\
  -H "Content-Type: application/json" \\
  -H "Authorization: Bearer YOUR_API_KEY" \\
  -d '{
    "url": "https://docs.example.com",
    "q": "getting started"
  }'`;

function isIndexNotFoundError(errorMsg: string): boolean {
  return (
    errorMsg.includes("index_not_found") ||
    errorMsg.includes("Index") && errorMsg.includes("not found") ||
    errorMsg.includes("Meilisearch returned 404")
  );
}

export default function SearchPage() {
  const router = useRouter();
  const [url, setUrl] = useState("https://scrapix.meilisearch.dev");
  const [query, setQuery] = useState("");
  const [limit, setLimit] = useState("20");
  const [loading, setLoading] = useState(false);
  const [crawling, setCrawling] = useState(false);
  const [result, setResult] = useState<MeilisearchSearchResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [indexNotFound, setIndexNotFound] = useState(false);
  const [runs, setRuns] = useState<RunEntry[]>([]);

  useEffect(() => {
    setRuns(loadRuns());
  }, []);

  const handleSearch = useCallback(async () => {
    if (!url.trim()) {
      toast.error("Please enter a URL");
      return;
    }
    if (!query.trim()) {
      toast.error("Please enter a search query");
      return;
    }

    setLoading(true);
    setResult(null);
    setError(null);
    setIndexNotFound(false);

    const start = performance.now();

    try {
      const data = await submitSearch({
        url,
        q: query,
        limit: parseInt(limit) || 20,
      });
      setResult(data);
      const duration = Math.round(performance.now() - start);
      const newRuns = saveRun({
        id: Math.random().toString(36).slice(2) + Date.now().toString(36),
        type: "search",
        url,
        duration_ms: duration,
        timestamp: new Date().toISOString(),
      });
      setRuns(newRuns);
    } catch (err) {
      const msg =
        err instanceof Error
          ? err.message
          : "Failed to fetch. Is the API running?";
      if (isIndexNotFoundError(msg)) {
        setIndexNotFound(true);
      } else {
        setError(msg);
      }
    }

    setLoading(false);
  }, [url, query, limit]);

  const handleQuickCrawl = useCallback(async () => {
    if (!url.trim()) return;

    setCrawling(true);
    try {
      const { job_id } = await createCrawl({
        start_urls: [url],
        max_depth: 3,
        max_pages: 100,
        sitemap: { enabled: true },
        meilisearch: {
          url: "",
          api_key: "",
        },
      });
      toast.success("Crawl started! Redirecting to job details...");
      router.push(`/dashboard/jobs/${job_id}`);
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to start crawl";
      toast.error(msg);
    }
    setCrawling(false);
  }, [url, router]);

  const handleReplay = (run: RunEntry) => {
    setUrl(run.url);
  };

  const indexUid = url.trim() ? urlToIndexUid(url) : "";

  return (
    <div className="flex flex-col gap-4 h-full">
      <UrlBar
        mode="search"
        url={url}
        onUrlChange={setUrl}
        onSubmit={handleSearch}
        loading={loading}
        historySlot={
          <div className="p-3 h-full">
            <HistoryPanel runs={runs} onReplay={handleReplay} typeFilter="search" />
          </div>
        }
      />

      <div className="grid grid-cols-1 lg:grid-cols-[minmax(280px,1fr)_3fr] gap-4 flex-1 min-h-0">
        <Card className="overflow-auto">
          <CardContent className="p-4">
            <div className="space-y-5">
              <div className="space-y-3">
                <Label className="text-xs text-muted-foreground uppercase tracking-wide">
                  Search Query
                </Label>
                <Input
                  placeholder="Enter your search query..."
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !loading) handleSearch();
                  }}
                  className="text-sm"
                />
              </div>

              <div className="space-y-3 border-t pt-4">
                <Label className="text-xs text-muted-foreground uppercase tracking-wide">
                  Parameters
                </Label>
                <div className="space-y-1.5">
                  <Label htmlFor="search-limit" className="text-sm font-medium">
                    Limit
                  </Label>
                  <Input
                    id="search-limit"
                    type="number"
                    min={1}
                    max={100}
                    value={limit}
                    onChange={(e) => setLimit(e.target.value)}
                    className="text-sm"
                  />
                </div>
              </div>

              {indexUid && (
                <div className="space-y-1.5 border-t pt-4">
                  <Label className="text-xs text-muted-foreground uppercase tracking-wide">
                    Resolved Index
                  </Label>
                  <p className="text-xs font-mono text-muted-foreground break-all">
                    {indexUid}
                  </p>
                </div>
              )}
            </div>
          </CardContent>
        </Card>

        <div className="flex-1 min-h-0">
          {loading && (
            <Card className="h-full">
              <CardContent className="p-6 space-y-4">
                <div className="flex items-center gap-2 text-muted-foreground">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  <span className="text-sm">Searching...</span>
                </div>
                {Array.from({ length: 5 }).map((_, i) => (
                  <div key={i} className="space-y-2 border-b pb-4">
                    <Skeleton className="h-4 w-3/4" />
                    <Skeleton className="h-3 w-1/2" />
                    <Skeleton className="h-3 w-full" />
                  </div>
                ))}
              </CardContent>
            </Card>
          )}

          {indexNotFound && (
            <Card className="border-orange-500/50">
              <CardContent className="p-6">
                <div className="flex flex-col items-center text-center gap-4 py-6">
                  <div className="rounded-full bg-orange-500/10 p-3">
                    <AlertCircle className="h-8 w-8 text-orange-500" />
                  </div>
                  <div className="space-y-2">
                    <h3 className="text-lg font-semibold">No index found for this website</h3>
                    <p className="text-sm text-muted-foreground max-w-md">
                      The index <code className="text-xs bg-muted px-1.5 py-0.5 rounded font-mono">{indexUid}</code> doesn&apos;t exist yet.
                      You need to crawl the website first so its content gets indexed.
                    </p>
                  </div>
                  <Button
                    onClick={handleQuickCrawl}
                    disabled={crawling}
                    className="gap-2 mt-2"
                  >
                    {crawling ? (
                      <Loader2 className="h-4 w-4 animate-spin" />
                    ) : (
                      <Layers className="h-4 w-4" />
                    )}
                    {crawling ? "Starting crawl..." : `Crawl ${url.trim() ? new URL(url).hostname : "website"}`}
                  </Button>
                  <p className="text-xs text-muted-foreground">
                    This will crawl up to 100 pages with depth 3 using default settings.
                  </p>
                </div>
              </CardContent>
            </Card>
          )}

          {error && !indexNotFound && (
            <Card className="border-destructive">
              <CardContent className="p-6">
                <p className="text-destructive text-sm">{error}</p>
              </CardContent>
            </Card>
          )}

          {result && (
            <Card className="h-full flex flex-col">
              <CardHeader className="pb-3 flex-none">
                <div className="flex items-center gap-3">
                  <CardTitle className="text-base">Search Results</CardTitle>
                  <Badge variant="secondary" className="font-mono">
                    <Hash className="mr-1 h-3 w-3" />
                    {result.estimatedTotalHits}
                  </Badge>
                  <Badge variant="outline" className="text-xs font-normal">
                    <Clock className="mr-1 h-3 w-3" />
                    {result.processingTimeMs}ms
                  </Badge>
                </div>
              </CardHeader>

              <CardContent className="flex-1 min-h-0 p-0">
                <ScrollArea className="h-full">
                  <div className="divide-y">
                    {result.hits.map((hit, i) => (
                      <SearchHitRow key={hit.uid ?? i} hit={hit} />
                    ))}
                    {result.hits.length === 0 && (
                      <div className="text-center text-muted-foreground py-12">
                        No results found for &quot;{query}&quot;
                      </div>
                    )}
                  </div>
                </ScrollArea>
              </CardContent>
            </Card>
          )}

          {!loading && !error && !result && !indexNotFound && (
            <Card className="h-full flex flex-col">
              <div className="px-6 pt-4 pb-2">
                <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                  API Example
                </p>
              </div>
              <div className="px-6 pb-6 flex-1 min-h-0">
                <CodeBlock code={SEARCH_EXAMPLE} lang="bash" />
              </div>
            </Card>
          )}
        </div>
      </div>
    </div>
  );
}

function SearchHitRow({ hit }: { hit: MeilisearchHit }) {
  const title =
    hit._formatted?.title || hit.title || hit._formatted?.h1 || hit.h1 || hit.url || "Untitled";
  const snippet =
    hit._formatted?.content ||
    hit._formatted?.markdown ||
    hit.content?.slice(0, 200) ||
    hit.markdown?.slice(0, 200) ||
    "";

  return (
    <div className="px-6 py-4 hover:bg-muted/30 transition-colors">
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          <h3
            className="text-sm font-medium truncate"
            dangerouslySetInnerHTML={{ __html: title }}
          />
          {hit.url && (
            <a
              href={hit.url}
              target="_blank"
              rel="noopener noreferrer"
              className="text-xs text-primary hover:underline flex items-center gap-1 mt-0.5"
            >
              <span className="truncate">{hit.url}</span>
              <ExternalLink className="h-3 w-3 shrink-0" />
            </a>
          )}
          {snippet && (
            <p
              className="text-xs text-muted-foreground mt-1.5 line-clamp-2"
              dangerouslySetInnerHTML={{ __html: snippet }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
