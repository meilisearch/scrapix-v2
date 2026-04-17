"use client";

import { memo, useState, useCallback, useRef, useEffect, type ReactNode } from "react";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Search,
  Globe,
  Clock,
  ExternalLink,
  ChevronLeft,
  ChevronRight,
  Loader2,
} from "lucide-react";
import { searchEngineIndex } from "@/lib/api";
import type { MeilisearchHit, MeilisearchSearchResponse } from "@/lib/api-types";
import { formatDistanceToNow } from "date-fns";

const HITS_PER_PAGE = 10;
const DEBOUNCE_MS = 120;

export interface IndexSearchPreviewProps {
  engineId: string;
  selectedIndex: string;
  /**
   * Rendered above the search input. Host pages pass the index selector
   * (Combobox or Select) along with any heading content here.
   */
  selector?: ReactNode;
}

export function IndexSearchPreview({
  engineId,
  selectedIndex,
  selector,
}: IndexSearchPreviewProps) {
  const [query, setQuery] = useState("");
  const [page, setPage] = useState(0);
  const [searchResult, setSearchResult] = useState<MeilisearchSearchResponse | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const abortRef = useRef<AbortController | null>(null);

  // Reset state when the selected index changes
  useEffect(() => {
    setQuery("");
    setSearchResult(null);
    setSearchError(null);
    setPage(0);
  }, [selectedIndex]);

  // Abort any in-flight request on unmount
  useEffect(() => {
    return () => abortRef.current?.abort();
  }, []);

  const performSearch = useCallback(
    async (q: string, offset: number) => {
      if (!q.trim() || !selectedIndex) {
        if (!q.trim()) {
          setSearchResult(null);
          setSearchError(null);
        }
        return;
      }
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;

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
        if (!controller.signal.aborted) {
          setSearchResult(result);
          setSearching(false);
        }
      } catch (err) {
        if (!controller.signal.aborted) {
          setSearchError(err instanceof Error ? err.message : "Search failed");
          setSearchResult(null);
          setSearching(false);
        }
      }
    },
    [engineId, selectedIndex]
  );

  useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    if (!query.trim()) {
      setSearchResult(null);
      setSearchError(null);
      setSearching(false);
      setPage(0);
      return;
    }
    debounceRef.current = setTimeout(() => {
      setPage(0);
      performSearch(query, 0);
    }, DEBOUNCE_MS);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [query, performSearch]);

  const handlePageChange = (newPage: number) => {
    setPage(newPage);
    performSearch(query, newPage * HITS_PER_PAGE);
    window.scrollTo({ top: 0, behavior: "smooth" });
  };

  const totalPages = searchResult
    ? Math.ceil(searchResult.estimatedTotalHits / HITS_PER_PAGE)
    : 0;
  const hasResults = searchResult && searchResult.hits.length > 0;
  const hasSearched = query.trim().length > 0;

  return (
    <div className="space-y-6">
      {selector && <div className="flex items-center gap-3">{selector}</div>}

      <div className="relative">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input
          placeholder="Search indexed content..."
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="pl-10 pr-10 h-12 text-base"
          autoFocus
        />
        {searching && (
          <Loader2 className="absolute right-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground animate-spin" />
        )}
      </div>

      {searchError && (
        <div className="rounded-lg border border-destructive/50 bg-destructive/5 p-4 text-sm text-destructive">
          {searchError}
        </div>
      )}

      {hasSearched && searchResult && !searching && (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <span>About {searchResult.estimatedTotalHits.toLocaleString()} results</span>
          <span className="text-muted-foreground/50">·</span>
          <span>{searchResult.processingTimeMs}ms</span>
        </div>
      )}

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
          <p>No results found for &ldquo;{query}&rdquo;</p>
          <p className="text-sm mt-1">Try different keywords or check another index.</p>
        </div>
      )}

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

const SearchHit = memo(function SearchHit({ hit }: { hit: MeilisearchHit }) {
  const formatted = hit._formatted;
  const titleHtml = formatted?.title;
  const titleText = hit.title || hit.url || "Untitled";
  const snippetHtml = formatted?.content;
  const url = hit.url;
  const domain = hit.domain;

  return (
    <div className="group">
      {url && (
        <div className="flex items-center gap-1.5 mb-0.5">
          <Globe className="h-3.5 w-3.5 text-muted-foreground flex-shrink-0" />
          <span className="text-sm text-muted-foreground truncate max-w-lg">{url}</span>
        </div>
      )}
      <a
        href={url}
        target="_blank"
        rel="noopener noreferrer"
        className="inline-flex items-center gap-1.5 group/link"
      >
        {titleHtml ? (
          <h3
            className="text-lg font-medium text-primary group-hover/link:underline leading-snug"
            dangerouslySetInnerHTML={{ __html: titleHtml }}
          />
        ) : (
          <h3 className="text-lg font-medium text-primary group-hover/link:underline leading-snug">
            {titleText}
          </h3>
        )}
        <ExternalLink className="h-3.5 w-3.5 text-muted-foreground opacity-0 group-hover/link:opacity-100 transition-opacity flex-shrink-0" />
      </a>
      {snippetHtml && (
        <p
          className="text-sm text-muted-foreground mt-1 leading-relaxed line-clamp-3 [&>mark]:bg-yellow-200 [&>mark]:text-foreground [&>mark]:dark:bg-yellow-900/60 [&>mark]:rounded-sm [&>mark]:px-0.5"
          dangerouslySetInnerHTML={{ __html: snippetHtml }}
        />
      )}
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
});
