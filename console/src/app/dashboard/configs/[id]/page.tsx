"use client";

import { useState } from "react";
import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  CardDescription,
} from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  ArrowLeft,
  Play,
  Clock,
  Save,
  Trash2,
  AlertCircle,
  ExternalLink,
  Copy,
} from "lucide-react";
import { HighlightedJson } from "@/components/highlighted-json";
import {
  fetchConfig,
  updateConfig,
  deleteConfig,
  triggerConfig,
} from "@/lib/api";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";
import {
  CrawlOptions,
  type CrawlState,
  defaultCrawlState,
} from "../../playground/crawl-options";
import {
  crawlStateToConfig,
  configToCrawlState,
  getStartUrls,
} from "@/lib/crawl-config-utils";
import { CronBuilder } from "@/components/cron-builder";

function ConfigPreview({ config }: { config: Record<string, unknown> }) {
  const jsonCode = JSON.stringify(config, null, 2);

  const formatValue = (v: unknown): string => {
    if (v == null) return "—";
    if (typeof v === "boolean") return v ? "Yes" : "No";
    if (Array.isArray(v)) return v.length === 0 ? "—" : v.join(", ");
    if (typeof v === "object") return JSON.stringify(v);
    return String(v);
  };

  const labelMap: Record<string, string> = {
    start_urls: "Start URLs",
    index_uid: "Index UID",
    max_depth: "Max Depth",
    max_pages: "Max Pages",
    crawler_type: "Crawler Type",
    allowed_domains: "Allowed Domains",
    index_strategy: "Index Strategy",
    keep_settings: "Keep Settings",
  };

  const sections: { title: string; entries: [string, string][] }[] = [];
  const crawl: [string, string][] = [];
  const meilisearch: [string, string][] = [];
  const patterns: [string, string][] = [];
  const other: [string, string][] = [];

  for (const [key, value] of Object.entries(config)) {
    if (typeof value === "object" && value !== null && !Array.isArray(value)) {
      const entries = Object.entries(value).filter(
        ([, v]) =>
          v != null &&
          v !== "" &&
          v !== false &&
          !(Array.isArray(v) && v.length === 0)
      );
      if (entries.length === 0) continue;

      if (key === "meilisearch") {
        for (const [k, v] of entries) {
          const display =
            k === "api_key" && typeof v === "string" && v.length > 4
              ? `${v.slice(0, 4)}${"*".repeat(Math.min(v.length - 4, 20))}`
              : formatValue(v);
          meilisearch.push([k.replace(/_/g, " "), display]);
        }
      } else if (key === "url_patterns") {
        for (const [k, v] of entries) {
          patterns.push([k.replace(/_/g, " "), formatValue(v)]);
        }
      } else {
        for (const [k, v] of entries) {
          other.push([`${key}.${k}`, formatValue(v)]);
        }
      }
      continue;
    }

    const display = formatValue(value);
    if (display !== "—") {
      crawl.push([labelMap[key] || key.replace(/_/g, " "), display]);
    }
  }

  if (crawl.length > 0) sections.push({ title: "Crawl", entries: crawl });
  if (patterns.length > 0)
    sections.push({ title: "URL Patterns", entries: patterns });
  if (meilisearch.length > 0)
    sections.push({ title: "Meilisearch", entries: meilisearch });
  if (other.length > 0) sections.push({ title: "Other", entries: other });

  return (
    <Tabs defaultValue="pretty">
      <TabsList>
        <TabsTrigger value="pretty">Pretty</TabsTrigger>
        <TabsTrigger value="json">JSON</TabsTrigger>
      </TabsList>
      <TabsContent value="pretty">
        <div className="space-y-4 pt-2">
          {sections.map(({ title, entries }) => (
            <div key={title}>
              <p className="text-xs font-medium text-muted-foreground uppercase tracking-wider mb-2">
                {title}
              </p>
              <div className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1">
                {entries.map(([label, value]) => (
                  <div key={label} className="contents">
                    <span className="text-sm text-muted-foreground capitalize">
                      {label}
                    </span>
                    <span className="text-sm font-mono truncate">{value}</span>
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      </TabsContent>
      <TabsContent value="json">
        <div className="bg-muted rounded-lg overflow-x-auto relative group">
          <Button
            variant="ghost"
            size="icon"
            className="absolute top-2 right-2 h-7 w-7 opacity-0 group-hover:opacity-100 transition-opacity"
            onClick={() => {
              navigator.clipboard.writeText(jsonCode);
              toast.success("Copied to clipboard");
            }}
          >
            <Copy className="h-3.5 w-3.5" />
          </Button>
          <HighlightedJson code={jsonCode} />
        </div>
      </TabsContent>
    </Tabs>
  );
}

export default function ConfigDetailPage() {
  const { id } = useParams<{ id: string }>();
  const router = useRouter();
  const queryClient = useQueryClient();

  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [startUrls, setStartUrls] = useState("");
  const [crawlState, setCrawlState] = useState<CrawlState>(defaultCrawlState);
  const [cronExpression, setCronExpression] = useState("");
  const [cronEnabled, setCronEnabled] = useState(false);
  const [saving, setSaving] = useState(false);
  const [triggerLoading, setTriggerLoading] = useState(false);
  const [showDelete, setShowDelete] = useState(false);

  const {
    data: config,
    isLoading,
    error,
  } = useQuery({
    queryKey: ["config", id],
    queryFn: () => fetchConfig(id),
    refetchInterval: 30_000,
  });

  const startEditing = () => {
    if (!config) return;
    setName(config.name);
    setDescription(config.description ?? "");
    const configObj = config.config as Record<string, unknown>;
    setStartUrls(getStartUrls(configObj).join("\n"));
    setCrawlState(configToCrawlState(configObj));
    setCronExpression(config.cron_expression ?? "");
    setCronEnabled(config.cron_enabled);
    setEditing(true);
  };

  const handleSave = async () => {
    const urls = startUrls
      .split("\n")
      .map((u) => u.trim())
      .filter((u) => u);

    if (urls.length === 0) {
      toast.error("At least one start URL is required");
      return;
    }

    const configPayload = crawlStateToConfig(crawlState, urls);

    setSaving(true);
    try {
      await updateConfig(id, {
        name: name.trim(),
        description: description.trim() || null,
        config: configPayload,
        cron_expression: cronExpression.trim() || null,
        cron_enabled: cronEnabled,
      });
      queryClient.invalidateQueries({ queryKey: ["config", id] });
      queryClient.invalidateQueries({ queryKey: ["configs"] });
      toast.success("Config updated");
      setEditing(false);
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : "Failed to update config"
      );
    } finally {
      setSaving(false);
    }
  };

  const handleTrigger = async () => {
    setTriggerLoading(true);
    try {
      const result = await triggerConfig(id);
      queryClient.invalidateQueries({ queryKey: ["config", id] });
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
      setTriggerLoading(false);
    }
  };

  const handleDelete = async () => {
    try {
      await deleteConfig(id);
      queryClient.invalidateQueries({ queryKey: ["configs"] });
      toast.success("Config deleted");
      router.push("/dashboard/configs");
    } catch {
      toast.error("Failed to delete config");
    } finally {
      setShowDelete(false);
    }
  };

  if (isLoading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  if (error || !config) {
    return (
      <Alert variant="destructive">
        <AlertCircle className="h-4 w-4" />
        <AlertDescription>
          {error instanceof Error
            ? error.message
            : "Config not found"}
        </AlertDescription>
      </Alert>
    );
  }

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="icon" asChild>
            <Link href="/dashboard/configs">
              <ArrowLeft className="h-4 w-4" />
            </Link>
          </Button>
          <div>
            <h2 className="text-2xl font-bold tracking-tight">
              {config.name}
            </h2>
            {config.description && (
              <p className="text-muted-foreground">{config.description}</p>
            )}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button
            onClick={handleTrigger}
            disabled={triggerLoading}
          >
            <Play className="h-4 w-4 mr-2" />
            {triggerLoading ? "Triggering..." : "Trigger Crawl"}
          </Button>
          {!editing && (
            <Button variant="outline" onClick={startEditing}>
              Edit
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setShowDelete(true)}
          >
            <Trash2 className="h-4 w-4 text-destructive" />
          </Button>
        </div>
      </div>

      {/* Info cards */}
      <div className="grid gap-3 grid-cols-2 md:grid-cols-4">
        <Card>
          <CardContent className="py-4">
            <p className="text-sm text-muted-foreground">Schedule</p>
            <div className="mt-1">
              {config.cron_expression ? (
                <div className="flex items-center gap-1.5">
                  <Badge
                    variant={config.cron_enabled ? "default" : "outline"}
                    className="gap-1"
                  >
                    <Clock className="h-3 w-3" />
                    {config.cron_enabled ? "Active" : "Paused"}
                  </Badge>
                </div>
              ) : (
                <span className="text-sm">Manual only</span>
              )}
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-4">
            <p className="text-sm text-muted-foreground">Cron</p>
            <p className="text-sm font-mono mt-1">
              {config.cron_expression ?? "None"}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-4">
            <p className="text-sm text-muted-foreground">Last Run</p>
            <p className="text-sm mt-1">
              {config.last_run_at
                ? formatDistanceToNow(new Date(config.last_run_at), {
                    addSuffix: true,
                  })
                : "Never"}
            </p>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="py-4">
            <p className="text-sm text-muted-foreground">Next Run</p>
            <p className="text-sm mt-1">
              {config.next_run_at
                ? formatDistanceToNow(new Date(config.next_run_at), {
                    addSuffix: true,
                  })
                : "N/A"}
            </p>
          </CardContent>
        </Card>
      </div>

      {/* Last job link */}
      {config.last_job_id && (
        <Card>
          <CardContent className="py-3 flex items-center justify-between">
            <div className="flex items-center gap-2 text-sm">
              <span className="text-muted-foreground">Last triggered job:</span>
              <code className="text-xs bg-muted px-1.5 py-0.5 rounded">
                {config.last_job_id.slice(0, 8)}
              </code>
            </div>
            <Button variant="ghost" size="sm" asChild>
              <Link href={`/dashboard/jobs/${config.last_job_id}`}>
                <ExternalLink className="h-3.5 w-3.5 mr-1.5" />
                View Job
              </Link>
            </Button>
          </CardContent>
        </Card>
      )}

      {/* Edit form or read-only config */}
      {editing ? (
        <Card>
          <CardHeader>
            <CardTitle>Edit Config</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="edit-name">Name</Label>
                <Input
                  id="edit-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                />
              </div>
              <div className="space-y-1.5">
                <Label htmlFor="edit-desc">Description</Label>
                <Input
                  id="edit-desc"
                  value={description}
                  onChange={(e) => setDescription(e.target.value)}
                  placeholder="Optional description"
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="edit-start-urls">Start URLs</Label>
              <Textarea
                id="edit-start-urls"
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

            <div className="space-y-3 rounded-lg border p-3">
              <div className="flex items-center justify-between">
                <div>
                  <Label
                    htmlFor="edit-cron-toggle"
                    className="text-sm font-medium"
                  >
                    Cron Schedule
                  </Label>
                  <p className="text-xs text-muted-foreground">
                    Automatically trigger this crawl on a schedule
                  </p>
                </div>
                <Switch
                  id="edit-cron-toggle"
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

            <div className="rounded-lg border p-3">
              <CrawlOptions state={crawlState} onChange={setCrawlState} />
            </div>

            <div className="flex gap-2 pt-2">
              <Button onClick={handleSave} disabled={saving}>
                <Save className="h-4 w-4 mr-2" />
                {saving ? "Saving..." : "Save Changes"}
              </Button>
              <Button variant="outline" onClick={() => setEditing(false)}>
                Cancel
              </Button>
            </div>
          </CardContent>
        </Card>
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>Crawl Configuration</CardTitle>
            <CardDescription>
              The full CrawlConfig JSON that will be used when this config is
              triggered
            </CardDescription>
          </CardHeader>
          <CardContent>
            <ConfigPreview config={config.config as Record<string, unknown>} />
          </CardContent>
        </Card>
      )}

      {/* Example trigger */}
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">API Trigger Example</CardTitle>
          <CardDescription>
            Trigger this config programmatically via the API
          </CardDescription>
        </CardHeader>
        <CardContent>
          <pre className="bg-muted rounded-lg p-4 overflow-x-auto text-xs font-mono">
{`curl -X POST https://scrapix.meilisearch.dev/configs/${config.id}/trigger \\
  -H "X-API-Key: sk_live_YOUR_KEY"`}
          </pre>
        </CardContent>
      </Card>

      {/* Delete confirmation */}
      <Dialog open={showDelete} onOpenChange={setShowDelete}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete Config</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete{" "}
              <span className="font-medium">{config.name}</span>? This will also
              remove any scheduled cron runs. This action is permanent.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowDelete(false)}>
              Cancel
            </Button>
            <Button variant="destructive" onClick={handleDelete}>
              Delete
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
