"use client";

import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { History } from "lucide-react";
import { formatDistanceToNow } from "date-fns";

export interface RunEntry {
  id: string;
  type: "scrape" | "crawl";
  url: string;
  status_code?: number;
  duration_ms?: number;
  timestamp: string;
}

const STORAGE_KEY = "scrapix-playground-runs";
const MAX_RUNS = 10;

export function loadRuns(): RunEntry[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

export function saveRun(run: RunEntry): RunEntry[] {
  const runs = [run, ...loadRuns()].slice(0, MAX_RUNS);
  localStorage.setItem(STORAGE_KEY, JSON.stringify(runs));
  return runs;
}

interface HistoryPanelProps {
  runs: RunEntry[];
  onReplay: (run: RunEntry) => void;
}

export function HistoryPanel({ runs, onReplay }: HistoryPanelProps) {
  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center gap-2 pb-3">
        <span className="text-sm font-medium">History</span>
        {runs.length > 0 && (
          <Badge variant="secondary" className="text-xs">
            {runs.length}
          </Badge>
        )}
      </div>

      {runs.length === 0 ? (
        <div className="flex flex-col items-center justify-center flex-1 text-muted-foreground gap-2 py-10">
          <History className="h-8 w-8 opacity-30" />
          <p className="text-xs">No runs yet</p>
        </div>
      ) : (
        <ScrollArea className="flex-1">
          <div className="space-y-1 pr-2">
            {runs.map((run) => (
              <button
                key={run.id}
                type="button"
                onClick={() => onReplay(run)}
                className="w-full text-left rounded-md px-2.5 py-2 hover:bg-muted/50 transition-colors cursor-pointer"
              >
                <div className="flex items-center gap-1.5 mb-1">
                  <Badge
                    variant={run.type === "scrape" ? "default" : "secondary"}
                    className="text-[10px] px-1.5 py-0"
                  >
                    {run.type}
                  </Badge>
                  {run.status_code && (
                    <Badge
                      variant={
                        run.status_code >= 200 && run.status_code < 400
                          ? "outline"
                          : "destructive"
                      }
                      className="text-[10px] px-1.5 py-0"
                    >
                      {run.status_code}
                    </Badge>
                  )}
                </div>
                <p className="text-xs font-mono text-muted-foreground truncate">
                  {run.url}
                </p>
                <div className="flex items-center gap-2 mt-1 text-[10px] text-muted-foreground">
                  {run.duration_ms != null && (
                    <span>{run.duration_ms}ms</span>
                  )}
                  <span>
                    {formatDistanceToNow(new Date(run.timestamp), {
                      addSuffix: true,
                    })}
                  </span>
                </div>
              </button>
            ))}
          </div>
        </ScrollArea>
      )}
    </div>
  );
}
