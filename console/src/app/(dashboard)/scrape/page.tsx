"use client";

import { useState, useEffect, useCallback } from "react";
import { Card, CardContent } from "@/components/ui/card";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { submitScrape, fetchServiceHealth } from "@/lib/api";
import type { ScrapeResult, ServiceStatus } from "@/lib/api-types";
import { UrlBar } from "../playground/url-bar";
import { ScrapeOptions, type ScrapeState } from "../playground/scrape-options";
import { ResultPanel } from "../playground/result-panel";
import { HistoryPanel, loadRuns, saveRun, type RunEntry } from "../playground/recent-runs";

export default function ScrapePage() {
  const [url, setUrl] = useState("https://example.com");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<ScrapeResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [runs, setRuns] = useState<RunEntry[]>([]);

  const [scrapeState, setScrapeState] = useState<ScrapeState>({
    formats: ["markdown", "metadata"],
    only_main_content: true,
    include_links: false,
    timeout_ms: "30000",
  });

  const [services, setServices] = useState<ServiceStatus[]>([]);

  useEffect(() => {
    setRuns(loadRuns());
  }, []);

  useEffect(() => {
    const poll = () =>
      fetchServiceHealth()
        .then((data) => setServices(data.services))
        .catch(() => {});
    poll();
    const interval = setInterval(poll, 10000);
    return () => clearInterval(interval);
  }, []);

  const handleScrape = useCallback(async () => {
    if (!url.trim()) {
      toast.error("Please enter a URL");
      return;
    }
    if (scrapeState.formats.length === 0) {
      toast.error("Select at least one output format");
      return;
    }

    setLoading(true);
    setResult(null);
    setError(null);

    try {
      const data = await submitScrape({
        url,
        formats: scrapeState.formats,
        only_main_content: scrapeState.only_main_content,
        include_links: scrapeState.include_links,
        timeout_ms: parseInt(scrapeState.timeout_ms) || 30000,
      });
      setResult(data);
      const newRuns = saveRun({
        id: Math.random().toString(36).slice(2) + Date.now().toString(36),
        type: "scrape",
        url,
        status_code: data.status_code,
        duration_ms: data.scrape_duration_ms,
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
  }, [url, scrapeState]);

  const handleReplay = (run: RunEntry) => {
    setUrl(run.url);
  };

  return (
    <div className="flex flex-col gap-4 h-[calc(100vh-6rem)]">
      {services.length > 0 && (
        <div className="flex items-center gap-3 px-1">
          {services.map((svc) => (
            <Badge
              key={svc.name}
              variant="outline"
              className="text-xs gap-1.5 font-normal"
            >
              <span
                className={`inline-block h-1.5 w-1.5 rounded-full ${
                  svc.status === "up"
                    ? "bg-green-500"
                    : svc.status === "idle"
                      ? "bg-yellow-500"
                      : "bg-gray-400"
                }`}
              />
              {svc.name}
            </Badge>
          ))}
        </div>
      )}

      <UrlBar
        mode="scrape"
        url={url}
        onUrlChange={setUrl}
        onSubmit={handleScrape}
        loading={loading}
      />

      <div className="grid grid-cols-1 lg:grid-cols-[minmax(280px,1fr)_2fr_minmax(220px,260px)] gap-4 flex-1 min-h-0">
        <Card className="overflow-auto">
          <CardContent className="p-4">
            <ScrapeOptions state={scrapeState} onChange={setScrapeState} />
          </CardContent>
        </Card>

        <Card className="overflow-hidden">
          <CardContent className="p-4 h-full">
            <ResultPanel
              result={result}
              crawlResult={null}
              mode="scrape"
              loading={loading}
              error={error}
            />
          </CardContent>
        </Card>

        <Card className="overflow-hidden">
          <CardContent className="p-4 h-full">
            <HistoryPanel runs={runs} onReplay={handleReplay} typeFilter="scrape" />
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
