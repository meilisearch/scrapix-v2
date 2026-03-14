"use client";

import { useState, useMemo } from "react";
import Link from "next/link";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetFooter,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Trash2,
  ExternalLink,
  AlertCircle,
  Search,
  Plus,
  Play,
  Clock,
  Settings2,
} from "lucide-react";
import { TableSkeleton } from "@/components/table-skeleton";
import { EmptyState } from "@/components/empty-state";
import {
  fetchConfigs,
  createConfig,
  deleteConfig,
  triggerConfig,
} from "@/lib/api";
import type { SavedConfig } from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import {
  CrawlOptions,
  type CrawlState,
  defaultCrawlState,
} from "../playground/crawl-options";
import { crawlStateToConfig } from "@/lib/crawl-config-utils";
import { CronBuilder } from "@/components/cron-builder";

export default function ConfigsPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<SavedConfig | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [triggerLoading, setTriggerLoading] = useState<string | null>(null);

  const {
    data: configs = [],
    isLoading,
    error,
  } = useQuery({
    queryKey: ["configs"],
    queryFn: fetchConfigs,
    refetchInterval: 30_000,
  });

  const filteredConfigs = useMemo(() => {
    if (!search.trim()) return configs;
    const q = search.toLowerCase();
    return configs.filter(
      (c) =>
        c.name.toLowerCase().includes(q) ||
        (c.description?.toLowerCase().includes(q) ?? false)
    );
  }, [configs, search]);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      await deleteConfig(deleteTarget.id);
      queryClient.invalidateQueries({ queryKey: ["configs"] });
      toast.success("Config deleted");
    } catch {
      toast.error("Failed to delete config");
    } finally {
      setDeleteTarget(null);
    }
  };

  const handleTrigger = async (config: SavedConfig) => {
    setTriggerLoading(config.id);
    try {
      const result = await triggerConfig(config.id);
      queryClient.invalidateQueries({ queryKey: ["configs"] });
      toast.success(
        <span>
          Crawl started.{" "}
          <Link href={`/dashboard/jobs/${result.job_id}`} className="underline">
            View job
          </Link>
        </span>
      );
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : "Failed to trigger crawl"
      );
    } finally {
      setTriggerLoading(null);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Configs</h2>
          <p className="text-muted-foreground">
            Saved crawl configurations with optional cron scheduling
          </p>
        </div>
        <div className="flex items-center gap-3">
          {!isLoading && configs.length > 0 && (
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search configs..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="pl-9 h-9 w-[200px]"
              />
            </div>
          )}
          <Button onClick={() => setShowCreate(true)}>
            <Plus className="h-4 w-4 mr-2" />
            New Config
          </Button>
        </div>
      </div>

      {error && (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>
            Could not load configs:{" "}
            {error instanceof Error ? error.message : "Unknown error"}
          </AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="pt-6 space-y-4">

          {isLoading ? (
            <TableSkeleton />
          ) : filteredConfigs.length === 0 ? (
            configs.length === 0 ? (
              <EmptyState
                message="No saved configs yet"
                action={
                  <Button
                    variant="outline"
                    onClick={() => setShowCreate(true)}
                  >
                    <Plus className="h-4 w-4 mr-2" />
                    Create your first config
                  </Button>
                }
              />
            ) : (
              <EmptyState message="No matching configs found." />
            )
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Schedule</TableHead>
                  <TableHead className="hidden md:table-cell">
                    Last Run
                  </TableHead>
                  <TableHead className="hidden sm:table-cell">
                    Created
                  </TableHead>
                  <TableHead className="w-[140px]" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredConfigs.map((config) => (
                  <TableRow key={config.id}>
                    <TableCell>
                      <Link
                        href={`/dashboard/configs/${config.id}`}
                        className="hover:underline text-primary font-medium text-sm"
                      >
                        {config.name}
                      </Link>
                      {config.description && (
                        <p className="text-xs text-muted-foreground truncate max-w-[250px]">
                          {config.description}
                        </p>
                      )}
                    </TableCell>
                    <TableCell>
                      {config.cron_expression ? (
                        <div className="flex items-center gap-1.5">
                          <Badge
                            variant={
                              config.cron_enabled ? "default" : "outline"
                            }
                            className="gap-1"
                          >
                            <Clock className="h-3 w-3" />
                            {config.cron_enabled ? "Active" : "Paused"}
                          </Badge>
                          <span className="text-xs font-mono text-muted-foreground">
                            {config.cron_expression}
                          </span>
                        </div>
                      ) : (
                        <span className="text-xs text-muted-foreground">
                          Manual only
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground hidden md:table-cell">
                      {config.last_run_at ? (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <span>
                              {formatDistanceToNow(
                                new Date(config.last_run_at),
                                { addSuffix: true }
                              )}
                            </span>
                          </TooltipTrigger>
                          <TooltipContent>
                            {new Date(config.last_run_at).toLocaleString()}
                          </TooltipContent>
                        </Tooltip>
                      ) : (
                        "Never"
                      )}
                    </TableCell>
                    <TableCell className="text-sm text-muted-foreground hidden sm:table-cell">
                      {formatDistanceToNow(new Date(config.created_at), {
                        addSuffix: true,
                      })}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-1">
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              variant="ghost"
                              size="icon"
                              onClick={() => handleTrigger(config)}
                              disabled={triggerLoading === config.id}
                            >
                              <Play
                                className={cn(
                                  "h-4 w-4",
                                  triggerLoading === config.id && "animate-pulse"
                                )}
                              />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>Trigger crawl</TooltipContent>
                        </Tooltip>
                        <Button variant="ghost" size="icon" asChild>
                          <Link href={`/dashboard/configs/${config.id}`}>
                            <ExternalLink className="h-4 w-4" />
                          </Link>
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => setDeleteTarget(config)}
                        >
                          <Trash2 className="h-4 w-4 text-destructive" />
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {/* Delete confirmation dialog */}
      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(open) => {
          if (!open) setDeleteTarget(null);
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Config</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete{" "}
              <span className="font-medium">{deleteTarget?.name}</span>? This
              will also remove any scheduled cron runs. This action is permanent.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleDelete}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Create config dialog */}
      <CreateConfigDialog
        open={showCreate}
        onOpenChange={setShowCreate}
        onCreated={() => {
          queryClient.invalidateQueries({ queryKey: ["configs"] });
        }}
      />
    </div>
  );
}

function CreateConfigDialog({
  open,
  onOpenChange,
  onCreated,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onCreated: () => void;
}) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [startUrls, setStartUrls] = useState("https://example.com");
  const [crawlState, setCrawlState] = useState<CrawlState>({
    ...defaultCrawlState,
    index_uid: "my-crawl",
    max_pages: "100",
    max_depth: "3",
  });
  const [cronExpression, setCronExpression] = useState("");
  const [cronEnabled, setCronEnabled] = useState(false);
  const [submitting, setSubmitting] = useState(false);

  const handleSubmit = async () => {
    if (!name.trim()) {
      toast.error("Name is required");
      return;
    }

    const urls = startUrls
      .split("\n")
      .map((u) => u.trim())
      .filter((u) => u);

    if (urls.length === 0) {
      toast.error("At least one start URL is required");
      return;
    }

    const config = crawlStateToConfig(crawlState, urls);

    setSubmitting(true);
    try {
      await createConfig({
        name: name.trim(),
        description: description.trim() || undefined,
        config,
        cron_expression: cronExpression.trim() || undefined,
        cron_enabled: cronEnabled,
      });
      toast.success("Config created");
      onCreated();
      onOpenChange(false);
      // Reset
      setName("");
      setDescription("");
      setStartUrls("https://example.com");
      setCrawlState({
        ...defaultCrawlState,
        index_uid: "my-crawl",
        max_pages: "100",
        max_depth: "3",
      });
      setCronExpression("");
      setCronEnabled(false);
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : "Failed to create config"
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="sm:max-w-xl w-full flex flex-col px-6">
        <SheetHeader>
          <SheetTitle className="flex items-center gap-2">
            <Settings2 className="h-5 w-5" />
            New Crawl Config
          </SheetTitle>
          <SheetDescription>
            Save a crawl configuration for reuse. Optionally add a cron schedule
            to trigger it automatically.
          </SheetDescription>
        </SheetHeader>

        <ScrollArea className="flex-1 -mx-6 overflow-hidden">
          <div className="space-y-6 px-6 pb-4">
            <div className="space-y-2">
              <Label htmlFor="config-name">Name</Label>
              <Input
                id="config-name"
                placeholder="My daily docs crawl"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="config-desc">Description</Label>
              <Input
                id="config-desc"
                placeholder="Crawls the docs site nightly"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="start-urls">Start URLs</Label>
              <Textarea
                id="start-urls"
                placeholder="https://example.com"
                value={startUrls}
                onChange={(e) => setStartUrls(e.target.value)}
                rows={2}
                className="font-mono text-xs"
              />
              <p className="text-xs text-muted-foreground">
                One URL per line
              </p>
            </div>

            {/* Cron Schedule */}
            <div className="space-y-3 rounded-lg border p-4">
              <div className="flex items-center justify-between">
                <div>
                  <Label htmlFor="cron-toggle" className="text-sm font-medium">
                    Cron Schedule
                  </Label>
                  <p className="text-xs text-muted-foreground">
                    Automatically trigger this crawl on a schedule
                  </p>
                </div>
                <Switch
                  id="cron-toggle"
                  checked={cronEnabled}
                  onCheckedChange={setCronEnabled}
                />
              </div>
              {cronEnabled && (
                <CronBuilder
                  value={cronExpression}
                  onChange={setCronExpression}
                />
              )}
            </div>

            {/* Crawl Options */}
            <div className="rounded-lg border p-4">
              <CrawlOptions state={crawlState} onChange={setCrawlState} />
            </div>
          </div>
        </ScrollArea>

        <SheetFooter className="flex-row justify-end gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting ? "Creating..." : "Create Config"}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
