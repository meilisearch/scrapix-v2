"use client";

import { useState, useMemo } from "react";
import Link from "next/link";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
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
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Trash2,
  ExternalLink,
  AlertCircle,
  RefreshCw,
  Search,
  Plus,
  Play,
  Clock,
  Settings2,
} from "lucide-react";
import {
  fetchConfigs,
  createConfig,
  deleteConfig,
  triggerConfig,
} from "@/lib/api";
import type { SavedConfig } from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";
import { cn } from "@/lib/utils";

export default function ConfigsPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<SavedConfig | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [triggerLoading, setTriggerLoading] = useState<string | null>(null);

  const {
    data: configs = [],
    isLoading,
    isFetching,
    error,
    dataUpdatedAt,
  } = useQuery({
    queryKey: ["configs"],
    queryFn: fetchConfigs,
    refetchInterval: 30_000,
  });

  const handleManualRefresh = () => {
    queryClient.invalidateQueries({ queryKey: ["configs"] });
  };

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
          <Link href={`/jobs/${result.job_id}`} className="underline">
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
        <Button onClick={() => setShowCreate(true)}>
          <Plus className="h-4 w-4 mr-2" />
          New Config
        </Button>
      </div>

      {error && (
        <Card className="border-destructive">
          <CardContent className="py-4 flex items-center gap-3">
            <AlertCircle className="h-5 w-5 text-destructive" />
            <p className="text-sm text-destructive">
              Could not load configs:{" "}
              {error instanceof Error ? error.message : "Unknown error"}
            </p>
          </CardContent>
        </Card>
      )}

      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <CardTitle>Saved Configs</CardTitle>
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              {dataUpdatedAt > 0 && (
                <span>
                  Updated{" "}
                  {formatDistanceToNow(new Date(dataUpdatedAt), {
                    addSuffix: true,
                  })}
                </span>
              )}
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7"
                onClick={handleManualRefresh}
                disabled={isFetching}
              >
                <RefreshCw
                  className={cn(
                    "h-3.5 w-3.5",
                    isFetching && "animate-spin"
                  )}
                />
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          {!isLoading && configs.length > 0 && (
            <div className="relative sm:ml-auto sm:w-fit">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search configs..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="pl-9 h-9 w-full sm:w-[200px]"
              />
            </div>
          )}

          {isLoading ? (
            <div className="space-y-3">
              {Array.from({ length: 5 }).map((_, i) => (
                <div key={i} className="flex items-center gap-4 py-2">
                  <Skeleton className="h-4 w-32" />
                  <Skeleton className="h-5 w-16 rounded-full" />
                  <Skeleton className="h-4 w-24" />
                  <Skeleton className="h-4 w-20 ml-auto" />
                </div>
              ))}
            </div>
          ) : filteredConfigs.length === 0 ? (
            <div className="text-center py-12">
              {configs.length === 0 ? (
                <div className="space-y-3">
                  <p className="text-muted-foreground">No saved configs yet</p>
                  <Button
                    variant="outline"
                    onClick={() => setShowCreate(true)}
                  >
                    <Plus className="h-4 w-4 mr-2" />
                    Create your first config
                  </Button>
                </div>
              ) : (
                <p className="text-muted-foreground">
                  No matching configs found.
                </p>
              )}
            </div>
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
                        href={`/configs/${config.id}`}
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
                        <span
                          title={new Date(
                            config.last_run_at
                          ).toLocaleString()}
                        >
                          {formatDistanceToNow(
                            new Date(config.last_run_at),
                            { addSuffix: true }
                          )}
                        </span>
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
                        <Button
                          variant="ghost"
                          size="icon"
                          onClick={() => handleTrigger(config)}
                          disabled={triggerLoading === config.id}
                          title="Trigger crawl"
                        >
                          <Play
                            className={cn(
                              "h-4 w-4",
                              triggerLoading === config.id && "animate-pulse"
                            )}
                          />
                        </Button>
                        <Button variant="ghost" size="icon" asChild>
                          <Link href={`/configs/${config.id}`}>
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
  const [configJson, setConfigJson] = useState(
    JSON.stringify(
      {
        start_urls: ["https://example.com"],
        index_uid: "my-crawl",
        max_depth: 3,
        max_pages: 100,
      },
      null,
      2
    )
  );
  const [cronExpression, setCronExpression] = useState("");
  const [cronEnabled, setCronEnabled] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [jsonError, setJsonError] = useState<string | null>(null);

  const handleSubmit = async () => {
    if (!name.trim()) {
      toast.error("Name is required");
      return;
    }

    let parsed;
    try {
      parsed = JSON.parse(configJson);
    } catch {
      setJsonError("Invalid JSON");
      return;
    }
    setJsonError(null);

    setSubmitting(true);
    try {
      await createConfig({
        name: name.trim(),
        description: description.trim() || undefined,
        config: parsed,
        cron_expression: cronExpression.trim() || undefined,
        cron_enabled: cronEnabled,
      });
      toast.success("Config created");
      onCreated();
      onOpenChange(false);
      // Reset
      setName("");
      setDescription("");
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
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <Settings2 className="h-5 w-5" />
            New Crawl Config
          </DialogTitle>
          <DialogDescription>
            Save a crawl configuration for reuse. Optionally add a cron schedule
            to trigger it automatically.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="config-name">Name</Label>
            <Input
              id="config-name"
              placeholder="My daily docs crawl"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="config-desc">Description (optional)</Label>
            <Input
              id="config-desc"
              placeholder="Crawls the docs site nightly"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="config-json">Crawl Config (JSON)</Label>
            <Textarea
              id="config-json"
              value={configJson}
              onChange={(e) => {
                setConfigJson(e.target.value);
                setJsonError(null);
              }}
              rows={8}
              className="font-mono text-xs"
            />
            {jsonError && (
              <p className="text-xs text-destructive">{jsonError}</p>
            )}
          </div>

          <div className="space-y-3 rounded-lg border p-3">
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
              <div className="space-y-1.5">
                <Label htmlFor="cron-expr">Cron Expression</Label>
                <Input
                  id="cron-expr"
                  placeholder="0 2 * * * (daily at 2am)"
                  value={cronExpression}
                  onChange={(e) => setCronExpression(e.target.value)}
                  className="font-mono text-sm"
                />
                <p className="text-xs text-muted-foreground">
                  Standard cron format: minute hour day month weekday
                </p>
              </div>
            )}
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting ? "Creating..." : "Create Config"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
