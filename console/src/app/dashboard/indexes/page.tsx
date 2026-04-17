"use client";

import { useState, useEffect, Suspense } from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Database,
  ChevronsUpDown,
  Check,
  Server,
  Layers,
} from "lucide-react";
import { fetchEngines, fetchEngineIndexes } from "@/lib/api";
import { IndexSearchPreview } from "@/components/index-search-preview";
import { cn } from "@/lib/utils";

function IndexesPageInner() {
  const router = useRouter();
  const searchParams = useSearchParams();
  const uidParam = searchParams.get("uid") ?? "";

  const { data: engines, isLoading: enginesLoading } = useQuery({
    queryKey: ["engines"],
    queryFn: fetchEngines,
  });

  const defaultEngine =
    engines?.find((e) => e.is_default) ?? engines?.[0] ?? null;

  const engineId = defaultEngine?.id;

  const { data: indexes = [], isLoading: indexesLoading } = useQuery({
    queryKey: ["engine-indexes", engineId],
    queryFn: () => fetchEngineIndexes(engineId!),
    enabled: !!engineId,
  });

  const [comboOpen, setComboOpen] = useState(false);

  // URL (?uid=) is the source of truth. Derive the selected index each render.
  const selectedIndex =
    indexes.find((i) => i.uid === uidParam)?.uid ?? indexes[0]?.uid ?? "";

  // Sync the URL to the derived selection when indexes have finished loading
  // and either ?uid= is missing or doesn't match any known index. This avoids
  // the page rendering with a stale/invalid ?uid= and populates ?uid= on first visit.
  useEffect(() => {
    if (indexesLoading) return;
    if (indexes.length === 0) return;
    const uidMatches = indexes.some((i) => i.uid === uidParam);
    if (!uidParam || !uidMatches) {
      router.replace(
        `/dashboard/indexes?uid=${encodeURIComponent(selectedIndex)}`,
        { scroll: false }
      );
    }
  }, [indexesLoading, indexes, uidParam, selectedIndex, router]);

  if (enginesLoading || (engineId && indexesLoading)) {
    return (
      <div className="max-w-3xl mx-auto space-y-6 pt-8">
        {defaultEngine && <Header engine={defaultEngine} />}
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-12 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    );
  }

  // No default engine
  if (!defaultEngine) {
    return (
      <div className="max-w-3xl mx-auto">
        <div className="text-center py-16 text-muted-foreground">
          <Server className="h-10 w-10 mx-auto mb-4 opacity-50" />
          <h2 className="text-xl font-semibold text-foreground mb-1">
            No Meilisearch engine configured
          </h2>
          <p className="text-sm mb-6">
            Connect a Meilisearch engine to browse and search indexed content.
          </p>
          <Button asChild>
            <Link href="/dashboard/settings">Go to Settings</Link>
          </Button>
        </div>
      </div>
    );
  }

  // No indexes on the default engine
  if (indexes.length === 0) {
    return (
      <div className="max-w-3xl mx-auto space-y-6">
        <Header engine={defaultEngine} />
        <div className="text-center py-16 text-muted-foreground">
          <Layers className="h-10 w-10 mx-auto mb-4 opacity-50" />
          <h2 className="text-xl font-semibold text-foreground mb-1">
            No indexes yet
          </h2>
          <p className="text-sm mb-6">
            Start a crawl to index content and make it searchable here.
          </p>
          <Button asChild>
            <Link href="/dashboard/crawl">Start a crawl</Link>
          </Button>
        </div>
      </div>
    );
  }

  const selector = (
    <>
      <Header engine={defaultEngine} />
      <Popover open={comboOpen} onOpenChange={setComboOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="outline"
            role="combobox"
            aria-expanded={comboOpen}
            className="w-[260px] justify-between"
          >
            <span className="flex items-center gap-2 min-w-0">
              <Database className="h-4 w-4 text-muted-foreground shrink-0" />
              <span className="truncate">
                {selectedIndex || "Select index"}
              </span>
            </span>
            <ChevronsUpDown className="h-4 w-4 opacity-50 shrink-0" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[260px] p-0" align="end">
          <Command>
            <CommandInput placeholder="Search indexes..." />
            <CommandList>
              <CommandEmpty>No indexes found.</CommandEmpty>
              <CommandGroup>
                {indexes.map((idx) => (
                  <CommandItem
                    key={idx.uid}
                    value={idx.uid}
                    onSelect={() => {
                      router.replace(
                        `/dashboard/indexes?uid=${encodeURIComponent(idx.uid)}`,
                        { scroll: false }
                      );
                      setComboOpen(false);
                    }}
                  >
                    <Check
                      className={cn(
                        "mr-2 h-4 w-4",
                        selectedIndex === idx.uid ? "opacity-100" : "opacity-0"
                      )}
                    />
                    <span className="truncate">{idx.uid}</span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
    </>
  );

  return (
    <div className="max-w-3xl mx-auto">
      <IndexSearchPreview
        engineId={defaultEngine.id}
        selectedIndex={selectedIndex}
        selector={selector}
      />
    </div>
  );
}

function Header({
  engine,
}: {
  engine: { name: string; url: string };
}) {
  return (
    <div className="flex-1 min-w-0">
      <h2 className="text-xl font-bold tracking-tight">Indexes</h2>
      <p className="text-sm text-muted-foreground truncate">
        {engine.name} · {engine.url}
      </p>
    </div>
  );
}

export default function IndexesPage() {
  return (
    <Suspense>
      <IndexesPageInner />
    </Suspense>
  );
}
