"use client";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Loader2, Play, History } from "lucide-react";

interface UrlBarProps {
  mode: "scrape" | "crawl" | "map";
  url: string;
  onUrlChange: (url: string) => void;
  onSubmit: () => void;
  loading: boolean;
  historySlot?: React.ReactNode;
}

export function UrlBar({
  mode,
  url,
  onUrlChange,
  onSubmit,
  loading,
  historySlot,
}: UrlBarProps) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-start gap-2 rounded-lg border bg-card p-2">
        {historySlot && (
          <Popover>
            <PopoverTrigger asChild>
              <Button variant="ghost" size="icon" className="shrink-0">
                <History className="h-4 w-4" />
              </Button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-72 p-0">
              <div className="h-80">{historySlot}</div>
            </PopoverContent>
          </Popover>
        )}
        {mode === "crawl" ? (
          <Textarea
            placeholder={"https://scrapix.meilisearch.dev\nhttps://scrapix.meilisearch.dev/docs"}
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !loading) {
                e.preventDefault();
                onSubmit();
              }
            }}
            rows={2}
            className="flex-1 font-mono text-sm min-h-0 resize-none"
          />
        ) : (
          <Input
            type="url"
            placeholder="https://scrapix.meilisearch.dev"
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !loading) onSubmit();
            }}
            className="flex-1 font-mono text-sm"
          />
        )}

        <Button
          onClick={onSubmit}
          disabled={loading}
          className="shrink-0 gap-2"
        >
          {loading ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Play className="h-4 w-4" />
          )}
          {mode === "scrape" ? "Scrape" : mode === "map" ? "Map" : "Start Crawl"}
        </Button>
      </div>

      <p className="text-xs text-muted-foreground px-1">
        {mode === "scrape"
          ? "Fetch and extract content from a single page."
          : mode === "map"
            ? "Discover all URLs on a website via sitemaps and crawling."
            : "Crawl a website by following links. Add one URL per line for multiple start URLs."}
      </p>
    </div>
  );
}
