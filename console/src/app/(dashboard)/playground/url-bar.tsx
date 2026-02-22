"use client";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Loader2, Play, Globe, Layers } from "lucide-react";
import { cn } from "@/lib/utils";

interface UrlBarProps {
  mode: "scrape" | "crawl";
  onModeChange: (mode: "scrape" | "crawl") => void;
  url: string;
  onUrlChange: (url: string) => void;
  onSubmit: () => void;
  loading: boolean;
}

export function UrlBar({
  mode,
  onModeChange,
  url,
  onUrlChange,
  onSubmit,
  loading,
}: UrlBarProps) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-start gap-2 rounded-lg border bg-card p-2">
        {/* Segmented mode toggle */}
        <div className="flex shrink-0 rounded-md border bg-muted p-0.5">
          <button
            onClick={() => onModeChange("scrape")}
            className={cn(
              "flex items-center gap-1.5 rounded-sm px-3 py-1.5 text-sm font-medium transition-colors",
              mode === "scrape"
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            <Globe className="h-3.5 w-3.5" />
            Scrape
          </button>
          <button
            onClick={() => onModeChange("crawl")}
            className={cn(
              "flex items-center gap-1.5 rounded-sm px-3 py-1.5 text-sm font-medium transition-colors",
              mode === "crawl"
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground hover:text-foreground"
            )}
          >
            <Layers className="h-3.5 w-3.5" />
            Crawl
          </button>
        </div>

        {mode === "scrape" ? (
          <Input
            type="url"
            placeholder="https://example.com"
            value={url}
            onChange={(e) => onUrlChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !loading) onSubmit();
            }}
            className="flex-1 font-mono text-sm"
          />
        ) : (
          <Textarea
            placeholder={"https://example.com\nhttps://example.com/docs"}
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
          {mode === "scrape" ? "Scrape" : "Start Crawl"}
        </Button>
      </div>

      <p className="text-xs text-muted-foreground px-1">
        {mode === "scrape"
          ? "Fetch and extract content from a single page."
          : "Crawl a website by following links. Add one URL per line for multiple start URLs."}
      </p>
    </div>
  );
}
