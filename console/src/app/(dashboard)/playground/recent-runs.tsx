"use client";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ChevronDown } from "lucide-react";
import { useState } from "react";
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

interface RecentRunsProps {
  runs: RunEntry[];
  onReplay: (run: RunEntry) => void;
}

export function RecentRuns({ runs, onReplay }: RecentRunsProps) {
  const [open, setOpen] = useState(false);

  if (runs.length === 0) return null;

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <Card>
        <CollapsibleTrigger asChild>
          <CardHeader className="cursor-pointer py-3 px-4">
            <Button
              variant="ghost"
              className="w-full justify-between p-0 h-auto hover:bg-transparent"
            >
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium">Recent Runs</span>
                <Badge variant="secondary" className="text-xs">
                  {runs.length}
                </Badge>
              </div>
              <ChevronDown
                className={`h-4 w-4 text-muted-foreground transition-transform ${
                  open ? "rotate-180" : ""
                }`}
              />
            </Button>
          </CardHeader>
        </CollapsibleTrigger>
        <CollapsibleContent>
          <CardContent className="pt-0 px-4 pb-2">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-[70px]">Type</TableHead>
                  <TableHead>URL</TableHead>
                  <TableHead className="w-[80px]">Status</TableHead>
                  <TableHead className="w-[80px]">Duration</TableHead>
                  <TableHead className="w-[100px] text-right">Time</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {runs.map((run) => (
                  <TableRow
                    key={run.id}
                    className="cursor-pointer"
                    onClick={() => onReplay(run)}
                  >
                    <TableCell>
                      <Badge
                        variant={run.type === "scrape" ? "default" : "secondary"}
                        className="text-xs"
                      >
                        {run.type}
                      </Badge>
                    </TableCell>
                    <TableCell className="font-mono text-xs max-w-[300px] truncate">
                      {run.url}
                    </TableCell>
                    <TableCell>
                      {run.status_code && (
                        <Badge
                          variant={
                            run.status_code >= 200 && run.status_code < 400
                              ? "outline"
                              : "destructive"
                          }
                          className="text-xs"
                        >
                          {run.status_code}
                        </Badge>
                      )}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {run.duration_ms != null ? `${run.duration_ms}ms` : "—"}
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground text-right">
                      {formatDistanceToNow(new Date(run.timestamp), {
                        addSuffix: true,
                      })}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </CollapsibleContent>
      </Card>
    </Collapsible>
  );
}
