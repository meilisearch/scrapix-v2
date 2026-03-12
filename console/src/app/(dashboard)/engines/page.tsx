"use client";

import { useState, useMemo, useEffect } from "react";
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
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import {
  Trash2,
  AlertCircle,
  Search,
  Plus,
  Star,
  Pencil,
  ChevronRight,
  Database,
  Eye,
} from "lucide-react";
import Link from "next/link";
import { TableSkeleton } from "@/components/table-skeleton";
import { EmptyState } from "@/components/empty-state";
import {
  fetchEngines,
  createEngine,
  updateEngine,
  deleteEngine,
  setDefaultEngine,
  fetchEngineIndexes,
} from "@/lib/api";
import type {
  MeilisearchEngine,
  MeilisearchIndex,
} from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";
import { toast } from "sonner";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

export default function EnginesPage() {
  const queryClient = useQueryClient();
  const [search, setSearch] = useState("");
  const [deleteTarget, setDeleteTarget] = useState<MeilisearchEngine | null>(null);
  const [editTarget, setEditTarget] = useState<MeilisearchEngine | null>(null);
  const [showCreate, setShowCreate] = useState(false);

  const {
    data: engines = [],
    isLoading,
    error,
  } = useQuery({
    queryKey: ["engines"],
    queryFn: fetchEngines,
    refetchInterval: 30_000,
  });

  const filteredEngines = useMemo(() => {
    if (!search.trim()) return engines;
    const q = search.toLowerCase();
    return engines.filter(
      (e) =>
        e.name.toLowerCase().includes(q) ||
        e.url.toLowerCase().includes(q)
    );
  }, [engines, search]);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    try {
      await deleteEngine(deleteTarget.id);
      queryClient.invalidateQueries({ queryKey: ["engines"] });
      toast.success("Engine deleted");
    } catch {
      toast.error("Failed to delete engine");
    } finally {
      setDeleteTarget(null);
    }
  };

  const handleSetDefault = async (engine: MeilisearchEngine) => {
    try {
      await setDefaultEngine(engine.id);
      queryClient.invalidateQueries({ queryKey: ["engines"] });
      toast.success(`${engine.name} set as default`);
    } catch {
      toast.error("Failed to set default engine");
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">Engines</h2>
          <p className="text-muted-foreground">
            Saved Meilisearch instances for crawl indexing
          </p>
        </div>
        <div className="flex items-center gap-3">
          {!isLoading && engines.length > 0 && (
            <div className="relative">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <Input
                placeholder="Search engines..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="pl-9 h-9 w-[200px]"
              />
            </div>
          )}
          <Button onClick={() => setShowCreate(true)}>
            <Plus className="h-4 w-4 mr-2" />
            Add Engine
          </Button>
        </div>
      </div>

      {error && (
        <Alert variant="destructive">
          <AlertCircle className="h-4 w-4" />
          <AlertDescription>
            Could not load engines:{" "}
            {error instanceof Error ? error.message : "Unknown error"}
          </AlertDescription>
        </Alert>
      )}

      <Card>
        <CardContent className="pt-6 space-y-4">

          {isLoading ? (
            <TableSkeleton
              rows={3}
              columns={["h-4 w-32", "h-4 w-48", "h-5 w-16 rounded-full", "h-4 w-20 ml-auto"]}
            />
          ) : filteredEngines.length === 0 ? (
            engines.length === 0 ? (
              <EmptyState
                message="No engines saved yet"
                action={
                  <Button
                    variant="outline"
                    onClick={() => setShowCreate(true)}
                  >
                    <Plus className="h-4 w-4 mr-2" />
                    Add your first engine
                  </Button>
                }
              />
            ) : (
              <EmptyState message="No matching engines found." />
            )
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-8" />
                  <TableHead>Name</TableHead>
                  <TableHead>URL</TableHead>
                  <TableHead className="hidden sm:table-cell">Created</TableHead>
                  <TableHead className="w-[140px]" />
                </TableRow>
              </TableHeader>
              <TableBody>
                {filteredEngines.map((engine) => (
                  <EngineRow
                    key={engine.id}
                    engine={engine}
                    onEdit={() => setEditTarget(engine)}
                    onDelete={() => setDeleteTarget(engine)}
                    onSetDefault={() => handleSetDefault(engine)}
                  />
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
            <DialogTitle>Delete Engine</DialogTitle>
            <DialogDescription>
              Are you sure you want to delete{" "}
              <span className="font-medium">{deleteTarget?.name}</span>? This
              action is permanent.
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

      {/* Create engine dialog */}
      <EngineFormDialog
        open={showCreate}
        onOpenChange={setShowCreate}
        onSaved={() => {
          queryClient.invalidateQueries({ queryKey: ["engines"] });
        }}
      />

      {/* Edit engine dialog */}
      <EngineFormDialog
        open={editTarget !== null}
        onOpenChange={(open) => {
          if (!open) setEditTarget(null);
        }}
        engine={editTarget ?? undefined}
        onSaved={() => {
          queryClient.invalidateQueries({ queryKey: ["engines"] });
          setEditTarget(null);
        }}
      />
    </div>
  );
}

function EngineRow({
  engine,
  onEdit,
  onDelete,
  onSetDefault,
}: {
  engine: MeilisearchEngine;
  onEdit: () => void;
  onDelete: () => void;
  onSetDefault: () => void;
}) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <TableRow>
        <TableCell className="px-2">
          <Collapsible open={open} onOpenChange={setOpen}>
            <CollapsibleTrigger asChild>
              <Button variant="ghost" size="icon" className="h-7 w-7">
                <ChevronRight
                  className={cn(
                    "h-4 w-4 transition-transform",
                    open && "rotate-90"
                  )}
                />
              </Button>
            </CollapsibleTrigger>
          </Collapsible>
        </TableCell>
        <TableCell>
          <div className="flex items-center gap-2">
            <span className="font-medium text-sm">{engine.name}</span>
            {engine.is_default && (
              <Badge variant="default" className="gap-1 text-xs">
                <Star className="h-3 w-3" />
                Default
              </Badge>
            )}
          </div>
        </TableCell>
        <TableCell>
          <span className="text-sm font-mono text-muted-foreground">
            {engine.url}
          </span>
        </TableCell>
        <TableCell className="text-sm text-muted-foreground hidden sm:table-cell">
          {formatDistanceToNow(new Date(engine.created_at), {
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
                  asChild
                >
                  <Link href={`/engines/${engine.id}/preview`}>
                    <Eye className="h-4 w-4" />
                  </Link>
                </Button>
              </TooltipTrigger>
              <TooltipContent>Search preview</TooltipContent>
            </Tooltip>
            {!engine.is_default && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon"
                    onClick={onSetDefault}
                  >
                    <Star className="h-4 w-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent>Set as default</TooltipContent>
              </Tooltip>
            )}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  onClick={onEdit}
                >
                  <Pencil className="h-4 w-4" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Edit</TooltipContent>
            </Tooltip>
            <Button
              variant="ghost"
              size="icon"
              onClick={onDelete}
            >
              <Trash2 className="h-4 w-4 text-destructive" />
            </Button>
          </div>
        </TableCell>
      </TableRow>
      {open && (
        <TableRow>
          <TableCell colSpan={5} className="p-0">
            <IndexList engineId={engine.id} />
          </TableCell>
        </TableRow>
      )}
    </>
  );
}

function IndexList({ engineId }: { engineId: string }) {
  const { data: indexes, isLoading, error } = useQuery({
    queryKey: ["engine-indexes", engineId],
    queryFn: () => fetchEngineIndexes(engineId),
  });

  if (isLoading) {
    return (
      <div className="px-8 py-4 space-y-2">
        <Skeleton className="h-4 w-48" />
        <Skeleton className="h-4 w-36" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="px-8 py-4 flex items-center gap-2 text-sm text-destructive">
        <AlertCircle className="h-4 w-4" />
        {error instanceof Error ? error.message : "Failed to fetch indexes"}
      </div>
    );
  }

  if (!indexes || indexes.length === 0) {
    return (
      <div className="px-8 py-4 text-sm text-muted-foreground">
        No indexes found on this engine.
      </div>
    );
  }

  return (
    <div className="px-8 py-3">
      <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium mb-2">
        Indexes ({indexes.length})
      </p>
      <div className="space-y-1.5">
        {indexes.map((idx) => (
          <div
            key={idx.uid}
            className="flex items-center gap-3 rounded-md bg-muted/50 px-3 py-2"
          >
            <Database className="h-3.5 w-3.5 text-muted-foreground" />
            <span className="text-sm font-mono font-medium">{idx.uid}</span>
            {idx.primaryKey && (
              <span className="text-xs text-muted-foreground">
                pk: {idx.primaryKey}
              </span>
            )}
            <span className="text-xs text-muted-foreground ml-auto">
              Updated{" "}
              {formatDistanceToNow(new Date(idx.updatedAt), {
                addSuffix: true,
              })}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function EngineFormDialog({
  open,
  onOpenChange,
  engine,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  engine?: MeilisearchEngine;
  onSaved: () => void;
}) {
  const isEdit = !!engine;
  const [name, setName] = useState(engine?.name ?? "");
  const [url, setUrl] = useState(engine?.url ?? "");
  const [apiKey, setApiKey] = useState(engine?.api_key ?? "");
  const [isDefault, setIsDefault] = useState(engine?.is_default ?? false);
  const [submitting, setSubmitting] = useState(false);

  const engineId = engine?.id;

  // Reset form when engine changes
  useEffect(() => {
    setName(engine?.name ?? "");
    setUrl(engine?.url ?? "");
    setApiKey(engine?.api_key ?? "");
    setIsDefault(engine?.is_default ?? false);
  }, [engine]);

  const handleSubmit = async () => {
    if (!name.trim()) {
      toast.error("Name is required");
      return;
    }
    if (!url.trim()) {
      toast.error("URL is required");
      return;
    }

    setSubmitting(true);
    try {
      if (isEdit && engineId) {
        await updateEngine(engineId, {
          name: name.trim(),
          url: url.trim(),
          api_key: apiKey,
        });
        toast.success("Engine updated");
      } else {
        await createEngine({
          name: name.trim(),
          url: url.trim(),
          api_key: apiKey || undefined,
          is_default: isDefault || undefined,
        });
        toast.success("Engine created");
      }
      onSaved();
      onOpenChange(false);
      if (!isEdit) {
        setName("");
        setUrl("");
        setApiKey("");
        setIsDefault(false);
      }
    } catch (err) {
      toast.error(
        err instanceof Error ? err.message : `Failed to ${isEdit ? "update" : "create"} engine`
      );
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="sm:max-w-md w-full flex flex-col">
        <SheetHeader>
          <SheetTitle className="flex items-center gap-2">
            <Database className="h-5 w-5" />
            {isEdit ? "Edit Engine" : "Add Engine"}
          </SheetTitle>
          <SheetDescription>
            {isEdit
              ? "Update the Meilisearch engine connection details."
              : "Save a Meilisearch instance for reuse in crawl configurations."}
          </SheetDescription>
        </SheetHeader>

        <div className="flex-1 space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="engine-name">Name</Label>
            <Input
              id="engine-name"
              placeholder="Production Meilisearch"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="engine-url">URL</Label>
            <Input
              id="engine-url"
              placeholder="http://localhost:7700"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              className="font-mono text-sm"
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="engine-key">API Key (optional)</Label>
            <Input
              id="engine-key"
              placeholder="masterKey"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              className="font-mono text-sm"
            />
          </div>

          {!isEdit && (
            <div className="flex items-center justify-between rounded-lg border p-3">
              <div>
                <Label htmlFor="engine-default" className="text-sm font-medium">
                  Set as default
                </Label>
                <p className="text-xs text-muted-foreground">
                  Auto-selected in the playground crawl form
                </p>
              </div>
              <Switch
                id="engine-default"
                checked={isDefault}
                onCheckedChange={setIsDefault}
              />
            </div>
          )}
        </div>

        <SheetFooter className="flex-row justify-end gap-2">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting
              ? isEdit
                ? "Saving..."
                : "Creating..."
              : isEdit
                ? "Save Changes"
                : "Add Engine"}
          </Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  );
}
