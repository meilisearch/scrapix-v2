"use client";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import {
  Copy,
  ExternalLink,
  Globe,
  CheckCircle2,
  XCircle,
  ArrowRight,
  Loader2,
  FileText,
  AlertCircle,
} from "lucide-react";
import { toast } from "sonner";
import { useEffect, useState, useCallback } from "react";
import Link from "next/link";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { codeToHtml } from "shiki";
import type { ScrapeResult, Job } from "@/lib/api-types";
import { fetchJobStatus } from "@/lib/api";

interface ResultPanelProps {
  result: ScrapeResult | null;
  crawlResult: { job_id: string; status: string; message?: string } | null;
  mode: "scrape" | "crawl";
  loading: boolean;
  error: string | null;
}

export function ResultPanel({
  result,
  crawlResult,
  mode,
  loading,
  error,
}: ResultPanelProps) {
  if (loading) {
    return <LoadingState />;
  }

  if (error) {
    return <ErrorState error={error} />;
  }

  if (mode === "crawl" && crawlResult) {
    return <CrawlResultState result={crawlResult} />;
  }

  if (mode === "scrape" && result) {
    return <ScrapeResultState result={result} />;
  }

  return <EmptyState />;
}

function EmptyState() {
  return (
    <div className="flex flex-col items-center justify-center h-full text-muted-foreground gap-3 py-20">
      <Globe className="h-10 w-10 opacity-40" />
      <p className="text-sm">Send a request to see results</p>
    </div>
  );
}

function LoadingState() {
  return (
    <div className="space-y-4 p-4">
      <div className="flex items-center gap-3">
        <Skeleton className="h-6 w-16 rounded-full" />
        <Skeleton className="h-4 w-24" />
        <Skeleton className="h-6 w-12 rounded-full" />
      </div>
      <Skeleton className="h-8 w-full" />
      <div className="space-y-2 pt-2">
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-[90%]" />
        <Skeleton className="h-4 w-[95%]" />
        <Skeleton className="h-4 w-[80%]" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-[85%]" />
        <Skeleton className="h-4 w-[70%]" />
        <Skeleton className="h-4 w-[92%]" />
      </div>
    </div>
  );
}

function ErrorState({ error }: { error: string }) {
  return (
    <div className="flex flex-col items-center justify-center h-full gap-3 py-20">
      <XCircle className="h-10 w-10 text-destructive opacity-60" />
      <p className="text-sm text-destructive font-medium">Request failed</p>
      <p className="text-xs text-muted-foreground max-w-md text-center">
        {error}
      </p>
    </div>
  );
}

function CrawlResultState({
  result,
}: {
  result: { job_id: string; status: string; message?: string };
}) {
  const [jobStatus, setJobStatus] = useState<Job | null>(null);

  const poll = useCallback(() => {
    fetchJobStatus(result.job_id)
      .then(setJobStatus)
      .catch(() => {});
  }, [result.job_id]);

  useEffect(() => {
    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  }, [poll]);

  const isRunning =
    jobStatus?.status === "running" || jobStatus?.status === "pending";
  const isCompleted = jobStatus?.status === "completed";
  const isFailed = jobStatus?.status === "failed";

  return (
    <div className="flex flex-col items-center justify-center h-full gap-6">
      {/* Status icon */}
      <div>
        {isRunning && (
          <Loader2 className="h-10 w-10 text-primary animate-spin" />
        )}
        {isCompleted && (
          <CheckCircle2 className="h-10 w-10 text-green-500" />
        )}
        {isFailed && (
          <XCircle className="h-10 w-10 text-destructive" />
        )}
        {!jobStatus && (
          <Globe className="h-10 w-10 text-primary opacity-70" />
        )}
      </div>

      {/* Title + badge */}
      <div className="flex flex-col items-center gap-2">
        <p className="text-lg font-semibold">
          {isRunning
            ? "Crawl in progress..."
            : isCompleted
              ? "Crawl completed"
              : isFailed
                ? "Crawl failed"
                : "Crawl job created"}
        </p>
        <Badge
          variant={isRunning ? "secondary" : isCompleted ? "default" : isFailed ? "destructive" : "outline"}
        >
          {isRunning && (
            <span className="relative mr-1.5 flex h-2 w-2">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-current opacity-75" />
              <span className="relative inline-flex h-2 w-2 rounded-full bg-current" />
            </span>
          )}
          {jobStatus?.status ?? result.status}
        </Badge>
      </div>

      {/* Counters */}
      {jobStatus && (
        <div className="flex items-center gap-6 text-sm text-muted-foreground">
          <span className="flex items-center gap-1.5">
            <FileText className="h-4 w-4" />
            <span className="font-medium text-foreground">{jobStatus.pages_crawled}</span> crawled
          </span>
          <span className="flex items-center gap-1.5">
            <CheckCircle2 className="h-4 w-4" />
            <span className="font-medium text-foreground">{jobStatus.pages_indexed}</span> indexed
          </span>
          <span className={cn("flex items-center gap-1.5", jobStatus.errors > 0 && "text-destructive")}>
            <AlertCircle className="h-4 w-4" />
            <span className="font-medium">{jobStatus.errors}</span> errors
          </span>
        </div>
      )}

      {/* Job ID */}
      <p className="text-xs text-muted-foreground font-mono">
        {result.job_id}
      </p>

      {/* Subtitle */}
      <p className="text-sm text-muted-foreground text-center max-w-xs">
        {isRunning
          ? "Pages will appear once the crawl finishes indexing."
          : isCompleted
            ? "Crawl complete. View full details to explore results."
            : isFailed
              ? "Check the job details for error information."
              : "Crawl job has been submitted."}
      </p>

      {/* Details link */}
      <Button variant="outline" asChild>
        <Link href={`/jobs/${result.job_id}`}>
          View details
          <ArrowRight className="ml-1.5 h-4 w-4" />
        </Link>
      </Button>
    </div>
  );
}

function ScrapeResultState({ result }: { result: ScrapeResult }) {
  const availableTabs: { value: string; label: string }[] = [];
  if (result.markdown) availableTabs.push({ value: "markdown", label: "Markdown" });
  if (result.metadata) availableTabs.push({ value: "metadata", label: "Metadata" });
  if (result.links && result.links.length > 0)
    availableTabs.push({ value: "links", label: "Links" });
  if (result.html) availableTabs.push({ value: "html", label: "HTML" });
  if (result.raw_html) availableTabs.push({ value: "rawhtml", label: "Raw HTML" });
  if (result.content) availableTabs.push({ value: "content", label: "Content" });
  availableTabs.push({ value: "json", label: "JSON" });

  const defaultTab = availableTabs[0]?.value ?? "json";

  const isSuccess = result.status_code >= 200 && result.status_code < 400;

  const copyText = (text: string) => {
    navigator.clipboard.writeText(text);
    toast.success("Copied to clipboard");
  };

  return (
    <div className="flex flex-col h-full">
      {/* Header row */}
      <div className="flex items-center gap-2 px-1 pb-3 flex-wrap">
        <Badge variant={isSuccess ? "default" : "destructive"}>
          {result.status_code}
        </Badge>
        <span className="text-xs text-muted-foreground">
          {result.scrape_duration_ms}ms
        </span>
        {result.language && (
          <Badge variant="outline" className="text-xs">
            {result.language}
          </Badge>
        )}
        <div className="ml-auto flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            onClick={() => copyText(JSON.stringify(result, null, 2))}
          >
            <Copy className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7"
            asChild
          >
            <a href={result.url} target="_blank" rel="noopener noreferrer">
              <ExternalLink className="h-3.5 w-3.5" />
            </a>
          </Button>
        </div>
      </div>

      {/* Tabs */}
      <Tabs defaultValue={defaultTab} className="flex-1 flex flex-col min-h-0">
        <TabsList className="w-full justify-start flex-wrap h-auto gap-1">
          {availableTabs.map((tab) => (
            <TabsTrigger
              key={tab.value}
              value={tab.value}
              className="text-xs"
            >
              {tab.label}
            </TabsTrigger>
          ))}
        </TabsList>

        <div className="flex-1 min-h-0 border rounded-md">
          {result.markdown && (
            <TabsContent value="markdown" className="h-full m-0">
              <ScrollArea className="h-full">
                <div className="prose prose-sm dark:prose-invert max-w-none p-4">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>
                    {result.markdown}
                  </ReactMarkdown>
                </div>
              </ScrollArea>
            </TabsContent>
          )}

          {result.metadata && (
            <TabsContent value="metadata" className="h-full m-0">
              <ScrollArea className="h-full">
                <MetadataView metadata={result.metadata} />
              </ScrollArea>
            </TabsContent>
          )}

          {result.links && result.links.length > 0 && (
            <TabsContent value="links" className="h-full m-0">
              <ScrollArea className="h-full">
                <div className="p-4 space-y-1">
                  {result.links.map((link, i) => (
                    <a
                      key={i}
                      href={link}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="flex items-center gap-2 text-sm font-mono text-muted-foreground hover:text-foreground transition-colors py-0.5"
                    >
                      <ExternalLink className="h-3 w-3 shrink-0" />
                      <span className="truncate">{link}</span>
                    </a>
                  ))}
                </div>
              </ScrollArea>
            </TabsContent>
          )}

          {result.html && (
            <TabsContent value="html" className="h-full m-0">
              <ScrollArea className="h-full">
                <pre className="whitespace-pre-wrap font-mono text-xs p-4">
                  {result.html}
                </pre>
              </ScrollArea>
            </TabsContent>
          )}

          {result.raw_html && (
            <TabsContent value="rawhtml" className="h-full m-0">
              <ScrollArea className="h-full">
                <pre className="whitespace-pre-wrap font-mono text-xs p-4">
                  {result.raw_html}
                </pre>
              </ScrollArea>
            </TabsContent>
          )}

          {result.content && (
            <TabsContent value="content" className="h-full m-0">
              <ScrollArea className="h-full">
                <pre className="whitespace-pre-wrap font-mono text-sm p-4 leading-relaxed">
                  {result.content}
                </pre>
              </ScrollArea>
            </TabsContent>
          )}

          <TabsContent value="json" className="h-full m-0">
            <ScrollArea className="h-full">
              <HighlightedJson code={JSON.stringify(result, null, 2)} />
            </ScrollArea>
          </TabsContent>
        </div>
      </Tabs>
    </div>
  );
}

function MetadataView({ metadata }: { metadata: NonNullable<ScrapeResult["metadata"]> }) {
  return (
    <div className="p-4 space-y-4">
      {metadata.title && (
        <MetaRow label="Title" value={metadata.title} />
      )}
      {metadata.description && (
        <MetaRow label="Description" value={metadata.description} />
      )}
      {metadata.author && (
        <MetaRow label="Author" value={metadata.author} />
      )}
      {metadata.canonical_url && (
        <MetaRow label="Canonical URL" value={metadata.canonical_url} mono />
      )}
      {metadata.published_date && (
        <MetaRow label="Published" value={metadata.published_date} />
      )}

      {metadata.keywords && metadata.keywords.length > 0 && (
        <div className="space-y-1">
          <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
            Keywords
          </p>
          <div className="flex flex-wrap gap-1">
            {metadata.keywords.map((kw, i) => (
              <Badge key={i} variant="secondary" className="text-xs">
                {kw}
              </Badge>
            ))}
          </div>
        </div>
      )}

      {metadata.open_graph &&
        Object.keys(metadata.open_graph).length > 0 && (
          <div className="space-y-2">
            <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
              Open Graph
            </p>
            <div className="space-y-1">
              {Object.entries(metadata.open_graph).map(([key, val]) => (
                <MetaRow key={key} label={key} value={val} small />
              ))}
            </div>
          </div>
        )}

      {metadata.twitter &&
        Object.keys(metadata.twitter).length > 0 && (
          <div className="space-y-2">
            <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
              Twitter
            </p>
            <div className="space-y-1">
              {Object.entries(metadata.twitter).map(([key, val]) => (
                <MetaRow key={key} label={key} value={val} small />
              ))}
            </div>
          </div>
        )}
    </div>
  );
}

function HighlightedJson({ code }: { code: string }) {
  const [html, setHtml] = useState<string>("");

  useEffect(() => {
    let cancelled = false;
    codeToHtml(code, {
      lang: "json",
      themes: { light: "github-light", dark: "github-dark" },
      defaultColor: false,
    }).then((result) => {
      if (!cancelled) setHtml(result);
    });
    return () => {
      cancelled = true;
    };
  }, [code]);

  if (!html) {
    return (
      <pre className="whitespace-pre-wrap font-mono text-xs p-4">{code}</pre>
    );
  }

  return (
    <div
      className="p-4 text-xs [&_pre]:!bg-transparent [&_code]:!bg-transparent [&_.shiki]:!bg-transparent"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
}

function MetaRow({
  label,
  value,
  mono,
  small,
}: {
  label: string;
  value: string;
  mono?: boolean;
  small?: boolean;
}) {
  return (
    <div className={small ? "flex gap-2 items-baseline" : "space-y-0.5"}>
      <p
        className={cn(
          "text-xs text-muted-foreground",
          small ? "shrink-0 w-24" : "uppercase tracking-wide font-medium"
        )}
      >
        {label}
      </p>
      <p
        className={cn(
          "text-sm",
          mono && "font-mono",
          small && "text-xs truncate"
        )}
      >
        {value}
      </p>
    </div>
  );
}
