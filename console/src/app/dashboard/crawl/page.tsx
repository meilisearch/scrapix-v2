"use client";

import { useState, useEffect, useCallback } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { toast } from "sonner";
import { createCrawl } from "@/lib/api";
import { UrlBar } from "../playground/url-bar";
import { CrawlOptions, type CrawlState, defaultCrawlState } from "../playground/crawl-options";
import { ResultPanel } from "../playground/result-panel";
import { HistoryPanel, loadRuns, saveRun, type RunEntry } from "../playground/recent-runs";
import { crawlStateToConfig } from "@/lib/crawl-config-utils";

export default function CrawlPage() {
  const [url, setUrl] = useState("https://scrapix.meilisearch.dev");
  const [loading, setLoading] = useState(false);
  const [crawlResult, setCrawlResult] = useState<{
    job_id: string;
    status: string;
    message?: string;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<RunEntry[]>([]);

  const [crawlState, setCrawlState] = useState<CrawlState>(defaultCrawlState);
  useEffect(() => {
    setRuns(loadRuns());
  }, []);

  const handleCrawl = useCallback(async () => {
    if (!url.trim()) {
      toast.error("Please enter a URL");
      return;
    }

    const urls = url
      .split("\n")
      .map((u) => u.trim())
      .filter((u) => u);

    setLoading(true);
    setCrawlResult(null);
    setError(null);

    const config = crawlStateToConfig(crawlState, urls);

    try {
      const data = await createCrawl(config);
      setCrawlResult({
        job_id: data.job_id,
        status: "created",
        message: `Crawl job submitted for ${urls.length} URL(s)`,
      });
      const newRuns = saveRun({
        id: Math.random().toString(36).slice(2) + Date.now().toString(36),
        type: "crawl",
        url: urls[0],
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
  }, [url, crawlState]);

  const handleReplay = (run: RunEntry) => {
    setUrl(run.url);
  };

  return (
    <div className="flex flex-col gap-4 h-full">
      <UrlBar
        mode="crawl"
        url={url}
        onUrlChange={setUrl}
        onSubmit={handleCrawl}
        loading={loading}
        historySlot={
          <div className="p-3 h-full">
            <HistoryPanel runs={runs} onReplay={handleReplay} typeFilter="crawl" />
          </div>
        }
      />

      <div className="grid grid-cols-1 lg:grid-cols-[minmax(280px,1fr)_3fr] gap-4 flex-1 min-h-0">
        <Card className="overflow-auto">
          <CardContent className="p-4">
            <CrawlOptions state={crawlState} onChange={setCrawlState} />
          </CardContent>
        </Card>

        <Card className="overflow-hidden">
          <CardContent className="p-4 h-full">
            <ResultPanel
              result={null}
              crawlResult={crawlResult}
              mode="crawl"
              loading={loading}
              error={error}
            />
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
