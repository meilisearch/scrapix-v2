"use client";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Loader2, Play } from "lucide-react";

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
    <div className="flex items-center gap-2 rounded-lg border bg-card p-2">
      <Select value={mode} onValueChange={(v) => onModeChange(v as "scrape" | "crawl")}>
        <SelectTrigger className="w-[120px] shrink-0">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="scrape">Scrape</SelectItem>
          <SelectItem value="crawl">Crawl</SelectItem>
        </SelectContent>
      </Select>

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

      <Button onClick={onSubmit} disabled={loading} className="shrink-0 gap-2">
        {loading ? (
          <Loader2 className="h-4 w-4 animate-spin" />
        ) : (
          <Play className="h-4 w-4" />
        )}
        {mode === "scrape" ? "Scrape" : "Start Crawl"}
      </Button>
    </div>
  );
}
