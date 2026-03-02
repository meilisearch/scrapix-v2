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
} from "lucide-react";
import {
  fetchConfig,
  updateConfig,
  deleteConfig,
  triggerConfig,
} from "@/lib/api";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";

export default function ConfigDetailPage() {
  const { id } = useParams<{ id: string }>();
  const router = useRouter();
  const queryClient = useQueryClient();

  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [configJson, setConfigJson] = useState("");
  const [cronExpression, setCronExpression] = useState("");
  const [cronEnabled, setCronEnabled] = useState(false);
  const [saving, setSaving] = useState(false);
  const [triggerLoading, setTriggerLoading] = useState(false);
  const [showDelete, setShowDelete] = useState(false);
  const [jsonError, setJsonError] = useState<string | null>(null);

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
    setConfigJson(JSON.stringify(config.config, null, 2));
    setCronExpression(config.cron_expression ?? "");
    setCronEnabled(config.cron_enabled);
    setJsonError(null);
    setEditing(true);
  };

  const handleSave = async () => {
    let parsed;
    try {
      parsed = JSON.parse(configJson);
    } catch {
      setJsonError("Invalid JSON");
      return;
    }
    setJsonError(null);

    setSaving(true);
    try {
      await updateConfig(id, {
        name: name.trim(),
        description: description.trim() || null,
        config: parsed,
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
      setTriggerLoading(false);
    }
  };

  const handleDelete = async () => {
    try {
      await deleteConfig(id);
      queryClient.invalidateQueries({ queryKey: ["configs"] });
      toast.success("Config deleted");
      router.push("/configs");
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
            <Link href="/configs">
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
              <Link href={`/jobs/${config.last_job_id}`}>
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

            <div className="space-y-1.5">
              <Label htmlFor="edit-config">Crawl Config (JSON)</Label>
              <Textarea
                id="edit-config"
                value={configJson}
                onChange={(e) => {
                  setConfigJson(e.target.value);
                  setJsonError(null);
                }}
                rows={12}
                className="font-mono text-xs"
              />
              {jsonError && (
                <p className="text-xs text-destructive">{jsonError}</p>
              )}
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
                <div className="space-y-1.5">
                  <Label htmlFor="edit-cron-expr">Cron Expression</Label>
                  <Input
                    id="edit-cron-expr"
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
            <pre className="bg-muted rounded-lg p-4 overflow-x-auto text-xs font-mono">
              {JSON.stringify(config.config, null, 2)}
            </pre>
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
{`curl -X POST http://localhost:8080/configs/${config.id}/trigger \\
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
