"use client";

import { useState, useCallback, useRef, useEffect } from "react";
import { useParams, useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import { Input } from "@/components/ui/input";
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
import {
  ArrowLeft,
  Search,
  Globe,
  Clock,
  ExternalLink,
  Database,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { fetchEngine, fetchEngineIndexes, searchEngineIndex } from "@/lib/api";
import type {
  MeilisearchEngine,
  MeilisearchIndex,
  MeilisearchHit,
  MeilisearchSearchResponse,
} from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";
import { cn } from "@/lib/utils";

const HITS_PER_PAGE = 10;

export default function EngineDemoPage() {
  const params = useParams();
  const router = useRouter();
  const engineId = params.id as string;

  const [selectedIndex, setSelectedIndex] = useState<string>("");
  const [query, setQuery] = useState("");
  const [submittedQuery, setSubmittedQuery] = useState("");
  const [page, setPage] = useState(0);
  const [searchResult, setSearchResult] = useState<MeilisearchSearchResponse | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const { data: engine, isLoading: engineLoading } = useQuery({
    queryKey: ["engine", engineId],
    queryFn: () => fetchEngine(engineId),
  });

  const { data: indexes = [], isLoading: indexesLoading } = useQuery({
    queryKey: ["engine-indexes", engineId],
    queryFn: () => fetchEngineIndexes(engineId),
  });

  // Auto-select first index
  useEffect(() => {
    if (indexes.length > 0 && !selectedIndex) {
      setSelectedIndex(indexes[0].uid);
    }
  }, [indexes, selectedIndex]);

  const handleSearch = useCallback(
    async (q: string, offset: number) => {
      if (!q.trim() || !selectedIndex) return;

      setSearching(true);
      setSearchError(null);
      try {
        const result = await searchEngineIndex(engineId, selectedIndex, {
          q: q.trim(),
          limit: HITS_PER_PAGE,
          offset,
          attributesToHighlight: ["title", "content", "h1", "h2", "h3"],
          attributesToCrop: ["content"],
          cropLength: 200,
          highlightPreTag: "<mark>",
          highlightPostTag: "</mark>",
        });
        setSearchResult(result);
      } catch (err) {
        setSearchError(err instanceof Error ? err.message : "Search failed");
        setSearchResult(null);
      } finally {
        setSearching(false);
      }
    },
    [engineId, selectedIndex]
  );

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setPage(0);
    setSubmittedQuery(query);
    handleSearch(query, 0);
  };

  const handlePageChange = (newPage: number) => {
    setPage(newPage);
    handleSearch(submittedQuery, newPage * HITS_PER_PAGE);
    window.scrollTo({ top: 0, behavior: "smooth" });
  };

  const totalPages = searchResult
    ? Math.ceil(searchResult.estimatedTotalHits / HITS_PER_PAGE)
    : 0;

  if (engineLoading || indexesLoading) {
    return (
      <div className="max-w-3xl mx-auto space-y-6 pt-8">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-12 w-full" />
        <Skeleton className="h-24 w-full" />
      </div>
    );
  }

  const hasResults = searchResult && searchResult.hits.length > 0;
  const hasSearched = submittedQuery.length > 0;

  return (
    <div className="max-w-3xl mx-auto space-y-6">
      {/* Header */}
      <div className="flex items-center gap-3">
        <Button
          variant="ghost"
          size="icon"
          onClick={() => router.push("/engines")}
        >
          <ArrowLeft className="h-4 w-4" />
        </Button>
        <div className="flex-1 min-w-0">
          <h2 className="text-xl font-bold tracking-tight truncate">
            {engine?.name ?? "Engine"} — Search Preview
          </h2>
          <p className="text-sm text-muted-foreground truncate">
            {engine?.url}
          </p>
        </div>
        {indexes.length > 1 && (
          <Select value={selectedIndex} onValueChange={(v) => {
            setSelectedIndex(v);
            setSearchResult(null);
            setSubmittedQuery("");
            setQuery("");
          }}>
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
      </div>

      {/* Search bar */}
      <form onSubmit={handleSubmit} className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input
          ref={inputRef}
          placeholder="Search indexed content..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="pl-10 pr-24 h-12 text-base"
          autoFocus
        />
        <Button
          type="submit"
          size="sm"
          className="absolute right-1.5 top-1/2 -translate-y-1/2"
          disabled={!query.trim() || !selectedIndex || searching}
        >
          {searching ? "Searching..." : "Search"}
        </Button>
      </form>

      {indexes.length === 0 && (
        <div className="text-center py-12 text-muted-foreground">
          <Database className="h-8 w-8 mx-auto mb-3 opacity-50" />
          <p>No indexes found on this engine.</p>
          <p className="text-sm mt-1">Crawl some content first to start searching.</p>
        </div>
      )}

      {/* Error */}
      {searchError && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/5 p-4 text-sm text-destructive">
          {searchError}
        </div>
      )}

      {/* Results meta */}
      {hasSearched && searchResult && (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <span>
            About {searchResult.estimatedTotalHits.toLocaleString()} results
          </span>
          <span className="text-muted-foreground/50">·</span>
          <span>{searchResult.processingTimeMs}ms</span>
        </div>
      )}

      {/* Results */}
      {searching && !searchResult && (
        <div className="space-y-8 pt-2">
          {Array.from({ length: 3 }).map((_, i) => (
            <div key={i} className="space-y-2">
              <Skeleton className="h-4 w-80" />
              <Skeleton className="h-5 w-96" />
              <Skeleton className="h-4 w-full" />
              <Skeleton className="h-4 w-3/4" />
            </div>
          ))}
        </div>
      )}

      {hasResults && (
        <div className="space-y-7">
          {searchResult.hits.map((hit, i) => (
            <SearchHit key={hit.uid ?? `hit-${i}`} hit={hit} />
          ))}
        </div>
      )}

      {hasSearched && searchResult && searchResult.hits.length === 0 && !searching && (
        <div className="text-center py-12 text-muted-foreground">
          <Search className="h-8 w-8 mx-auto mb-3 opacity-50" />
          <p>No results found for &ldquo;{submittedQuery}&rdquo;</p>
          <p className="text-sm mt-1">Try different keywords or check another index.</p>
        </div>
      )}

      {/* Pagination */}
      {totalPages > 1 && (
        <div className="flex items-center justify-center gap-2 pt-4 pb-8">
          <Button
            variant="outline"
            size="sm"
            disabled={page === 0}
            onClick={() => handlePageChange(page - 1)}
          >
            <ChevronLeft className="h-4 w-4 mr-1" />
            Previous
          </Button>
          <span className="text-sm text-muted-foreground px-3">
            Page {page + 1} of {totalPages}
          </span>
          <Button
            variant="outline"
            size="sm"
            disabled={page >= totalPages - 1}
            onClick={() => handlePageChange(page + 1)}
          >
            Next
            <ChevronRight className="h-4 w-4 ml-1" />
          </Button>
        </div>
      )}
    </div>
  );
}

function SearchHit({ hit }: { hit: MeilisearchHit }) {
  const formatted = hit._formatted;
  const title = formatted?.title || hit.title || hit.url || "Untitled";
  const snippet = formatted?.content || hit.content || "";
  const url = hit.url;
  const domain = hit.domain;

  return (
    <div className="group">
      {/* URL breadcrumb */}
      {url && (
        <div className="flex items-center gap-1.5 mb-0.5">
          <Globe className="h-3.5 w-3.5 text-muted-foreground flex-shrink-0" />
          <span className="text-sm text-muted-foreground truncate max-w-lg">
            {url}
          </span>
        </div>
      )}

      {/* Title */}
      <a
        href={url}
        target="_blank"
        rel="noopener noreferrer"
        className="inline-flex items-center gap-1.5 group/link"
      >
        <h3
          className="text-lg font-medium text-primary group-hover/link:underline leading-snug"
          dangerouslySetInnerHTML={{ __html: title }}
        />
        <ExternalLink className="h-3.5 w-3.5 text-muted-foreground opacity-0 group-hover/link:opacity-100 transition-opacity flex-shrink-0" />
      </a>

      {/* Snippet */}
      {snippet && (
        <p
          className="text-sm text-muted-foreground mt-1 leading-relaxed line-clamp-3 [&>mark]:bg-yellow-200 [&>mark]:text-foreground [&>mark]:dark:bg-yellow-900/60 [&>mark]:rounded-sm [&>mark]:px-0.5"
          dangerouslySetInnerHTML={{ __html: snippet }}
        />
      )}

      {/* Meta row */}
      <div className="flex items-center gap-3 mt-1.5">
        {domain && (
          <Badge variant="outline" className="text-xs font-normal gap-1">
            {domain}
          </Badge>
        )}
        {hit.language && (
          <Badge variant="outline" className="text-xs font-normal">
            {hit.language}
          </Badge>
        )}
        {hit.crawled_at && (
          <span className="text-xs text-muted-foreground flex items-center gap-1">
            <Clock className="h-3 w-3" />
            {formatDistanceToNow(new Date(hit.crawled_at), { addSuffix: true })}
          </span>
        )}
      </div>
    </div>
  );
}
