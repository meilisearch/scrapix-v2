"use client";

import { useState, useEffect } from "react";
import { useParams, useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ArrowLeft, Database } from "lucide-react";
import { fetchEngine, fetchEngineIndexes } from "@/lib/api";
import { IndexSearchPreview } from "@/components/index-search-preview";

export default function EngineDemoPage() {
  const params = useParams();
  const router = useRouter();
  const engineId = params.id as string;

  const [selectedIndex, setSelectedIndex] = useState<string>("");

  const { data: engine, isLoading: engineLoading } = useQuery({
    queryKey: ["engine", engineId],
    queryFn: () => fetchEngine(engineId),
  });

  const { data: indexes = [], isLoading: indexesLoading } = useQuery({
    queryKey: ["engine-indexes", engineId],
    queryFn: () => fetchEngineIndexes(engineId),
  });

  useEffect(() => {
    if (indexes.length > 0 && !selectedIndex) {
      setSelectedIndex(indexes[0].uid);
    }
  }, [indexes, selectedIndex]);

  if (engineLoading || indexesLoading) {
    return (
      <div className="max-w-3xl mx-auto space-y-6 pt-8">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-12 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    );
  }

  const selector = (
    <>
      <Button
        variant="ghost"
        size="icon"
        onClick={() => router.push("/dashboard/engines")}
      >
        <ArrowLeft className="h-4 w-4" />
      </Button>
      <div className="flex-1 min-w-0">
        <h2 className="text-xl font-bold tracking-tight truncate">
          {engine?.name ?? "Engine"} — Search Preview
        </h2>
        <p className="text-sm text-muted-foreground truncate">{engine?.url}</p>
      </div>
      {indexes.length > 1 && (
        <Select value={selectedIndex} onValueChange={setSelectedIndex}>
          <SelectTrigger className="w-[200px]">
            <Database className="h-4 w-4 mr-2 text-muted-foreground" />
            <SelectValue placeholder="Select index" />
          </SelectTrigger>
          <SelectContent>
            {indexes.map((idx) => (
              <SelectItem key={idx.uid} value={idx.uid}>
                {idx.uid}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      )}
      {indexes.length === 1 && (
        <Badge variant="secondary" className="gap-1.5">
          <Database className="h-3 w-3" />
          {indexes[0].uid}
        </Badge>
      )}
    </>
  );

  if (indexes.length === 0) {
    return (
      <div className="max-w-3xl mx-auto space-y-6">
        <div className="flex items-center gap-3">{selector}</div>
        <div className="text-center py-12 text-muted-foreground">
          <Database className="h-8 w-8 mx-auto mb-3 opacity-50" />
          <p>No indexes found on this engine.</p>
          <p className="text-sm mt-1">Crawl some content first to start searching.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="max-w-3xl mx-auto">
      <IndexSearchPreview
        engineId={engineId}
        selectedIndex={selectedIndex}
        selector={selector}
      />
    </div>
  );
}
