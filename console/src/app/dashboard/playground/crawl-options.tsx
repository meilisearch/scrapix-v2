"use client";

import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
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
import { Globe, Monitor, Plus, X, ChevronsUpDown, Check } from "lucide-react";
import { cn } from "@/lib/utils";

const SCHEMA_ORG_TYPES = [
  // Creative Works
  "Article", "NewsArticle", "BlogPosting", "TechArticle", "ScholarlyArticle",
  "WebPage", "WebSite", "CreativeWork",
  // Commerce
  "Product", "Offer", "AggregateOffer", "Review", "AggregateRating",
  // Organizations & People
  "Organization", "Corporation", "LocalBusiness", "Restaurant", "Person",
  // Events
  "Event", "BusinessEvent", "MusicEvent", "SportsEvent",
  // How-to & FAQ
  "FAQPage", "QAPage", "HowTo", "HowToStep",
  // Media
  "VideoObject", "ImageObject", "AudioObject", "MediaObject",
  // Jobs & Education
  "JobPosting", "Course", "EducationalOrganization",
  // Food
  "Recipe",
  // Software
  "SoftwareApplication", "MobileApplication", "WebApplication",
  // Navigation
  "BreadcrumbList", "SiteNavigationElement", "ItemList",
  // Medical
  "MedicalCondition", "Drug", "MedicalEntity",
  // Other
  "Book", "Movie", "MusicRecording", "Place", "Action",
  "Service", "Dataset", "DefinedTerm",
] as const;

export interface CrawlState {
  // General
  index_uid: string;
  source: string;
  max_depth: string;
  max_pages: string;
  crawler_type: "http" | "browser";
  sitemap_enabled: boolean;
  sitemap_urls: string;
  respect_robots: boolean;
  // Patterns
  allowed_domains: string;
  include_patterns: string;
  exclude_patterns: string;
  index_only_patterns: string;
  // Performance
  max_concurrent_requests: string;
  browser_pool_size: string;
  dns_concurrency: string;
  requests_per_second: string;
  requests_per_minute: string;
  per_domain_delay_ms: string;
  default_crawl_delay_ms: string;
  // Features
  feat_metadata: boolean;
  feat_markdown: boolean;
  feat_block_split: boolean;
  feat_schema: boolean;
  schema_only_types: string;
  schema_convert_dates: boolean;
  feat_custom_selectors: boolean;
  custom_selectors: string;
  feat_ai_extraction: boolean;
  ai_extraction_prompt: string;
  feat_ai_summary: boolean;
  // Indexing
  index_strategy: "update" | "replace";
  // Advanced
  headers: string;
  user_agents: string;
  proxy_urls: string;
  proxy_rotation: "round_robin" | "random" | "least_used";
  meilisearch_engine_id: string;
  meilisearch_url: string;
  meilisearch_api_key: string;
  meilisearch_primary_key: string;
  meilisearch_batch_size: string;
  meilisearch_keep_settings: boolean;
}

export const defaultCrawlState: CrawlState = {
  index_uid: "",
  source: "",
  max_depth: "",
  max_pages: "1000000",
  crawler_type: "http",
  sitemap_enabled: true,
  sitemap_urls: "",
  respect_robots: true,
  allowed_domains: "",
  include_patterns: "",
  exclude_patterns: "",
  index_only_patterns: "",
  max_concurrent_requests: "50",
  browser_pool_size: "5",
  dns_concurrency: "100",
  requests_per_second: "",
  requests_per_minute: "",
  per_domain_delay_ms: "100",
  default_crawl_delay_ms: "1000",
  feat_metadata: true,
  feat_markdown: true,
  feat_block_split: false,
  feat_schema: false,
  schema_only_types: "",
  schema_convert_dates: true,
  feat_custom_selectors: false,
  custom_selectors: "",
  feat_ai_extraction: false,
  ai_extraction_prompt: "",
  feat_ai_summary: false,
  index_strategy: "update",
  headers: "",
  user_agents: "",
  proxy_urls: "",
  proxy_rotation: "round_robin",
  meilisearch_engine_id: "",
  meilisearch_url: "http://localhost:7700",
  meilisearch_api_key: "masterKey",
  meilisearch_primary_key: "",
  meilisearch_batch_size: "1000",
  meilisearch_keep_settings: false,
};

interface CrawlOptionsProps {
  state: CrawlState;
  onChange: (state: CrawlState) => void;
}

function NumericInput({
  id,
  label,
  value,
  onChange,
  placeholder,
  min,
  max,
}: {
  id: string;
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  min?: string;
  max?: string;
}) {
  return (
    <div className="space-y-1.5">
      <Label htmlFor={id} className="text-sm font-medium">
        {label}
      </Label>
      <Input
        id={id}
        type="number"
        min={min}
        max={max}
        placeholder={placeholder}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}

function SwitchRow({
  id,
  label,
  description,
  checked,
  onCheckedChange,
}: {
  id: string;
  label: string;
  description?: string;
  checked: boolean;
  onCheckedChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between">
      <div>
        <Label htmlFor={id} className="text-sm font-medium">
          {label}
        </Label>
        {description && (
          <p className="text-xs text-muted-foreground">{description}</p>
        )}
      </div>
      <Switch id={id} checked={checked} onCheckedChange={onCheckedChange} />
    </div>
  );
}

function PatternList({
  label,
  description,
  placeholder,
  value,
  onChange,
}: {
  label: string;
  description: string;
  placeholder: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [input, setInput] = useState("");

  const items = value
    .split("\n")
    .map((s) => s.trim())
    .filter((s) => s);

  const addItem = () => {
    const trimmed = input.trim();
    if (!trimmed || items.includes(trimmed)) return;
    onChange([...items, trimmed].join("\n"));
    setInput("");
  };

  const removeItem = (index: number) => {
    onChange(items.filter((_, i) => i !== index).join("\n"));
  };

  return (
    <div className="space-y-2">
      <div>
        <Label className="text-sm font-medium">{label}</Label>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>

      {items.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {items.map((item, i) => (
            <Badge
              key={i}
              variant="secondary"
              className="gap-1 pl-2 pr-1 py-0.5 font-mono text-xs"
            >
              {item}
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="ml-0.5 h-4 w-4 rounded-full p-0 hover:bg-muted-foreground/20"
                onClick={() => removeItem(i)}
              >
                <X className="h-3 w-3" />
              </Button>
            </Badge>
          ))}
        </div>
      )}

      <div className="flex gap-1.5">
        <Input
          placeholder={placeholder}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addItem();
            }
          }}
          className="flex-1 font-mono text-xs"
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          className="shrink-0 h-9 w-9"
          onClick={addItem}
          disabled={!input.trim()}
        >
          <Plus className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}

function KeyValueList({
  label,
  description,
  keyPlaceholder,
  valuePlaceholder,
  value,
  onChange,
}: {
  label: string;
  description: string;
  keyPlaceholder: string;
  valuePlaceholder: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [key, setKey] = useState("");
  const [val, setVal] = useState("");

  const entries: [string, string][] = (() => {
    if (!value.trim()) return [];
    try {
      return Object.entries(JSON.parse(value)) as [string, string][];
    } catch {
      return [];
    }
  })();

  const addEntry = () => {
    const k = key.trim();
    const v = val.trim();
    if (!k || !v) return;
    const obj = Object.fromEntries(entries);
    obj[k] = v;
    onChange(JSON.stringify(obj));
    setKey("");
    setVal("");
  };

  const removeEntry = (k: string) => {
    const obj = Object.fromEntries(entries.filter(([ek]) => ek !== k));
    onChange(Object.keys(obj).length > 0 ? JSON.stringify(obj) : "");
  };

  return (
    <div className="space-y-2">
      <div>
        <Label className="text-sm font-medium">{label}</Label>
        <p className="text-xs text-muted-foreground">{description}</p>
      </div>

      {entries.length > 0 && (
        <div className="space-y-1">
          {entries.map(([k, v]) => (
            <div
              key={k}
              className="flex items-center gap-2 rounded-md bg-secondary/50 px-2.5 py-1.5"
            >
              <span className="text-xs font-medium shrink-0">{k}</span>
              <span className="text-muted-foreground text-xs">&rarr;</span>
              <span className="text-xs font-mono text-muted-foreground flex-1 truncate">
                {v}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="h-4 w-4 rounded-full p-0 hover:bg-muted-foreground/20 shrink-0"
                onClick={() => removeEntry(k)}
              >
                <X className="h-3 w-3" />
              </Button>
            </div>
          ))}
        </div>
      )}

      <div className="flex gap-1.5">
        <Input
          placeholder={keyPlaceholder}
          value={key}
          onChange={(e) => setKey(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addEntry();
            }
          }}
          className="flex-1 text-xs"
        />
        <Input
          placeholder={valuePlaceholder}
          value={val}
          onChange={(e) => setVal(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              addEntry();
            }
          }}
          className="flex-1 font-mono text-xs"
        />
        <Button
          type="button"
          variant="outline"
          size="icon"
          className="shrink-0 h-9 w-9"
          onClick={addEntry}
          disabled={!key.trim() || !val.trim()}
        >
          <Plus className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}

function SchemaTypePicker({
  value,
  onChange,
}: {
  value: string;
  onChange: (value: string) => void;
}) {
  const [open, setOpen] = useState(false);

  const selected = value
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s);

  const addType = (type: string) => {
    if (!selected.includes(type)) {
      onChange([...selected, type].join(", "));
    }
    setOpen(false);
  };

  const removeType = (type: string) => {
    onChange(selected.filter((t) => t !== type).join(", "));
  };

  return (
    <div className="space-y-2">
      <div>
        <Label className="text-sm font-medium">Only types</Label>
        <p className="text-xs text-muted-foreground">
          Select schema.org types to extract. Empty = all.
        </p>
      </div>

      {selected.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {selected.map((type) => (
            <Badge
              key={type}
              variant="secondary"
              className="gap-1 pl-2 pr-1 py-0.5 text-xs"
            >
              {type}
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className="ml-0.5 h-4 w-4 rounded-full p-0 hover:bg-muted-foreground/20"
                onClick={() => removeType(type)}
              >
                <X className="h-3 w-3" />
              </Button>
            </Badge>
          ))}
        </div>
      )}

      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <Button
            variant="outline"
            role="combobox"
            aria-expanded={open}
            className="w-full justify-between text-xs font-normal"
          >
            Add a schema type...
            <ChevronsUpDown className="ml-2 h-3.5 w-3.5 shrink-0 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="w-[--radix-popover-trigger-width] p-0" align="start">
          <Command>
            <CommandInput placeholder="Search types..." className="text-xs" />
            <CommandList>
              <CommandEmpty>No type found.</CommandEmpty>
              <CommandGroup className="max-h-[200px] overflow-auto">
                {SCHEMA_ORG_TYPES.map((type) => (
                  <CommandItem
                    key={type}
                    value={type}
                    onSelect={() => addType(type)}
                    className="text-xs"
                  >
                    <Check
                      className={cn(
                        "mr-2 h-3.5 w-3.5",
                        selected.includes(type) ? "opacity-100" : "opacity-0"
                      )}
                    />
                    {type}
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
    </div>
  );
}

export function CrawlOptions({ state, onChange }: CrawlOptionsProps) {
  const set = <K extends keyof CrawlState>(key: K, value: CrawlState[K]) =>
    onChange({ ...state, [key]: value });

  return (
    <Tabs defaultValue="general" className="h-full flex flex-col">
      <TabsList className="w-full">
        {["General", "Advanced"].map((tab) => (
          <TabsTrigger key={tab} value={tab.toLowerCase()} className="text-xs">
            {tab}
          </TabsTrigger>
        ))}
      </TabsList>

      {/* ── General (merged General + Features + Patterns) ── */}
      <TabsContent value="general" className="flex-1 pt-3">
        <ScrollArea className="h-full">
          <div className="space-y-5 pr-3">
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground uppercase tracking-wide">
                Crawler Type
              </Label>
              <ToggleGroup
                type="single"
                variant="outline"
                value={state.crawler_type}
                onValueChange={(v) => {
                  if (v) set("crawler_type", v as "http" | "browser");
                }}
                className="w-full"
              >
                <ToggleGroupItem
                  value="http"
                  className="flex-1 gap-1.5 data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
                >
                  <Globe className="h-3.5 w-3.5" />
                  HTTP
                </ToggleGroupItem>
                <ToggleGroupItem
                  value="browser"
                  className="flex-1 gap-1.5 data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
                >
                  <Monitor className="h-3.5 w-3.5" />
                  Browser (JS)
                </ToggleGroupItem>
              </ToggleGroup>
            </div>

            <SwitchRow
              id="sitemap"
              label="Use sitemaps"
              description="Discover and follow sitemaps"
              checked={state.sitemap_enabled}
              onCheckedChange={(v) => set("sitemap_enabled", v)}
            />

            {state.sitemap_enabled && (
              <div className="space-y-1.5 pl-1 border-l-2 border-primary/20 ml-1">
                <Label
                  htmlFor="sitemap-urls"
                  className="text-sm font-medium"
                >
                  Sitemap URLs
                </Label>
                <p className="text-xs text-muted-foreground">
                  Explicit sitemap URLs, one per line. Auto-discovered if empty.
                </p>
                <Textarea
                  id="sitemap-urls"
                  placeholder={"https://example.com/sitemap.xml"}
                  value={state.sitemap_urls}
                  onChange={(e) => set("sitemap_urls", e.target.value)}
                  rows={2}
                />
              </div>
            )}

            <SwitchRow
              id="robots"
              label="Respect robots.txt"
              description="Follow crawl directives"
              checked={state.respect_robots}
              onCheckedChange={(v) => set("respect_robots", v)}
            />

            {/* ── Patterns ── */}
            <div className="space-y-1 pt-3 border-t">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                URL Patterns
              </p>
            </div>

            <PatternList
              label="Allowed Domains"
              description="Auto-inferred from start URLs if empty."
              placeholder="example.com"
              value={state.allowed_domains}
              onChange={(v) => set("allowed_domains", v)}
            />

            <PatternList
              label="Include Patterns"
              description="Glob patterns for URLs to crawl."
              placeholder="*/docs/*"
              value={state.include_patterns}
              onChange={(v) => set("include_patterns", v)}
            />

            <PatternList
              label="Exclude Patterns"
              description="Glob patterns for URLs to skip."
              placeholder="*/login*"
              value={state.exclude_patterns}
              onChange={(v) => set("exclude_patterns", v)}
            />

            <PatternList
              label="Index-Only Patterns"
              description="Indexed but links are not followed."
              placeholder="*/archive/*"
              value={state.index_only_patterns}
              onChange={(v) => set("index_only_patterns", v)}
            />

            {/* ── Features ── */}
            <div className="space-y-1 pt-3 border-t">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                Features
              </p>
            </div>

            <SwitchRow
              id="feat-metadata"
              label="Metadata extraction"
              description="Title, description, OG tags, etc."
              checked={state.feat_metadata}
              onCheckedChange={(v) => set("feat_metadata", v)}
            />

            <SwitchRow
              id="feat-markdown"
              label="Markdown conversion"
              description="Convert page content to Markdown"
              checked={state.feat_markdown}
              onCheckedChange={(v) => set("feat_markdown", v)}
            />

            <SwitchRow
              id="feat-block-split"
              label="Block splitting"
              description="Split content into semantic blocks"
              checked={state.feat_block_split}
              onCheckedChange={(v) => set("feat_block_split", v)}
            />

            {/* Schema */}
            <SwitchRow
              id="feat-schema"
              label="Schema extraction"
              description="JSON-LD, Microdata, RDFa"
              checked={state.feat_schema}
              onCheckedChange={(v) => set("feat_schema", v)}
            />
            {state.feat_schema && (
              <div className="space-y-3 pl-1 border-l-2 border-primary/20 ml-1">
                <SchemaTypePicker
                  value={state.schema_only_types}
                  onChange={(v) => set("schema_only_types", v)}
                />
                <SwitchRow
                  id="schema-dates"
                  label="Convert dates"
                  checked={state.schema_convert_dates}
                  onCheckedChange={(v) => set("schema_convert_dates", v)}
                />
              </div>
            )}

            {/* Custom Selectors */}
            <SwitchRow
              id="feat-selectors"
              label="Custom CSS selectors"
              description="Extract content with CSS selectors"
              checked={state.feat_custom_selectors}
              onCheckedChange={(v) => set("feat_custom_selectors", v)}
            />
            {state.feat_custom_selectors && (
              <div className="pl-1 border-l-2 border-primary/20 ml-1">
                <KeyValueList
                  label="Selectors"
                  description="Map field names to CSS selectors."
                  keyPlaceholder="field"
                  valuePlaceholder=".css-selector"
                  value={state.custom_selectors}
                  onChange={(v) => set("custom_selectors", v)}
                />
              </div>
            )}

            {/* AI Extraction */}
            <SwitchRow
              id="feat-ai-extraction"
              label="AI extraction"
              description="Use LLM to extract structured data"
              checked={state.feat_ai_extraction}
              onCheckedChange={(v) => set("feat_ai_extraction", v)}
            />
            {state.feat_ai_extraction && (
              <div className="space-y-3 pl-1 border-l-2 border-primary/20 ml-1">
                <div className="space-y-1.5">
                  <Label htmlFor="ai-prompt" className="text-sm font-medium">
                    Prompt
                  </Label>
                  <Textarea
                    id="ai-prompt"
                    placeholder="Extract the product name, price, and description from this page."
                    value={state.ai_extraction_prompt}
                    onChange={(e) =>
                      set("ai_extraction_prompt", e.target.value)
                    }
                    rows={3}
                  />
                </div>
              </div>
            )}

            {/* AI Summary */}
            <SwitchRow
              id="feat-ai-summary"
              label="AI summary"
              description="Generate page summaries with LLM"
              checked={state.feat_ai_summary}
              onCheckedChange={(v) => set("feat_ai_summary", v)}
            />
          </div>
        </ScrollArea>
      </TabsContent>

      {/* ── Advanced ── */}
      <TabsContent value="advanced" className="flex-1 pt-3">
        <ScrollArea className="h-full">
          <div className="space-y-5 pr-3">
            {/* Crawl Limits */}
            <div className="space-y-1">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                Crawl Limits
              </p>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <NumericInput
                id="max-depth"
                label="Max Depth"
                value={state.max_depth}
                onChange={(v) => set("max_depth", v)}
                placeholder="Unlimited"
                min="0"
              />
              <NumericInput
                id="max-pages"
                label="Max Pages"
                value={state.max_pages}
                onChange={(v) => set("max_pages", v)}
                min="1"
              />
            </div>

            {/* Concurrency */}
            <div className="space-y-1 pt-3 border-t">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                Concurrency
              </p>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <NumericInput
                id="max-concurrent"
                label="Max Concurrent Requests"
                value={state.max_concurrent_requests}
                onChange={(v) => set("max_concurrent_requests", v)}
                min="1"
              />
              <NumericInput
                id="browser-pool"
                label="Browser Pool Size"
                value={state.browser_pool_size}
                onChange={(v) => set("browser_pool_size", v)}
                min="1"
              />
            </div>

            <NumericInput
              id="dns-concurrency"
              label="DNS Concurrency"
              value={state.dns_concurrency}
              onChange={(v) => set("dns_concurrency", v)}
              min="1"
            />

            {/* Rate Limiting */}
            <div className="space-y-1 pt-3 border-t">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                Rate Limiting
              </p>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <NumericInput
                id="rps"
                label="Requests / Second"
                value={state.requests_per_second}
                onChange={(v) => set("requests_per_second", v)}
                placeholder="No limit"
                min="0.1"
              />
              <NumericInput
                id="rpm"
                label="Requests / Minute"
                value={state.requests_per_minute}
                onChange={(v) => set("requests_per_minute", v)}
                placeholder="No limit"
                min="1"
              />
            </div>

            <div className="grid grid-cols-2 gap-3">
              <NumericInput
                id="domain-delay"
                label="Per-Domain Delay (ms)"
                value={state.per_domain_delay_ms}
                onChange={(v) => set("per_domain_delay_ms", v)}
                min="0"
              />
              <NumericInput
                id="crawl-delay"
                label="Default Crawl Delay (ms)"
                value={state.default_crawl_delay_ms}
                onChange={(v) => set("default_crawl_delay_ms", v)}
                min="0"
              />
            </div>

            {/* HTTP */}
            <div className="space-y-1 pt-3 border-t">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                HTTP
              </p>
            </div>

            <KeyValueList
              label="Custom Headers"
              description="Add HTTP headers sent with every request."
              keyPlaceholder="Header name"
              valuePlaceholder="Header value"
              value={state.headers}
              onChange={(v) => set("headers", v)}
            />

            <PatternList
              label="User Agents"
              description="Rotated per request."
              placeholder="Mozilla/5.0 (compatible; ScrapixBot/1.0)"
              value={state.user_agents}
              onChange={(v) => set("user_agents", v)}
            />

            {/* Proxy */}
            <div className="space-y-1 pt-3 border-t">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                Proxy
              </p>
            </div>

            <PatternList
              label="Proxy URLs"
              description="Add proxy endpoints. Rotated per request."
              placeholder="http://proxy1:8080"
              value={state.proxy_urls}
              onChange={(v) => set("proxy_urls", v)}
            />

            {state.proxy_urls.trim() && (
              <div className="space-y-1.5">
                <Label className="text-xs text-muted-foreground uppercase tracking-wide">
                  Rotation Strategy
                </Label>
                <ToggleGroup
                  type="single"
                  variant="outline"
                  value={state.proxy_rotation}
                  onValueChange={(v) => {
                    if (v)
                      set(
                        "proxy_rotation",
                        v as "round_robin" | "random" | "least_used"
                      );
                  }}
                  className="w-full"
                >
                  <ToggleGroupItem
                    value="round_robin"
                    className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
                  >
                    Round Robin
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="random"
                    className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
                  >
                    Random
                  </ToggleGroupItem>
                  <ToggleGroupItem
                    value="least_used"
                    className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
                  >
                    Least Used
                  </ToggleGroupItem>
                </ToggleGroup>
              </div>
            )}

            {/* Indexing */}
            <div className="space-y-1 pt-3 border-t">
              <p className="text-xs text-muted-foreground uppercase tracking-wide font-medium">
                Indexing
              </p>
              <p className="text-xs text-muted-foreground">
                Index UID and Meilisearch connection are auto-configured from your account settings.
              </p>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="source" className="text-sm font-medium">
                Source
              </Label>
              <p className="text-xs text-muted-foreground">
                Tag documents with a source identifier for multi-tenant indexing.
                Enables per-source filtering and deletion within a shared index.
              </p>
              <Input
                id="source"
                placeholder="Optional (defaults to domain)"
                value={state.source}
                onChange={(e) => set("source", e.target.value)}
                className="font-mono text-xs"
              />
            </div>

            <div className="space-y-1.5">
              <Label className="text-sm font-medium">Index Strategy</Label>
              <p className="text-xs text-muted-foreground">
                Update adds new documents or updates existing ones — unchanged pages are skipped using conditional HTTP headers. Replace creates a fresh index and swaps it atomically on completion (always re-crawls all pages).
              </p>
              <ToggleGroup
                type="single"
                variant="outline"
                value={state.index_strategy}
                onValueChange={(v) => {
                  if (v) set("index_strategy", v as "update" | "replace");
                }}
                className="w-full"
              >
                <ToggleGroupItem
                  value="update"
                  className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
                >
                  Update
                </ToggleGroupItem>
                <ToggleGroupItem
                  value="replace"
                  className="flex-1 text-xs data-[state=on]:bg-primary/10 data-[state=on]:text-primary data-[state=on]:border-primary/30"
                >
                  Replace
                </ToggleGroupItem>
              </ToggleGroup>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="space-y-1.5">
                <Label htmlFor="ms-pk" className="text-sm font-medium">
                  Primary Key
                </Label>
                <Input
                  id="ms-pk"
                  placeholder="Auto-detect"
                  value={state.meilisearch_primary_key}
                  onChange={(e) =>
                    set("meilisearch_primary_key", e.target.value)
                  }
                  className="font-mono text-xs"
                />
              </div>
              <NumericInput
                id="ms-batch"
                label="Batch Size"
                value={state.meilisearch_batch_size}
                onChange={(v) => set("meilisearch_batch_size", v)}
                min="1"
              />
            </div>

            <SwitchRow
              id="ms-keep-settings"
              label="Keep index settings"
              description="Don't overwrite existing index configuration"
              checked={state.meilisearch_keep_settings}
              onCheckedChange={(v) => set("meilisearch_keep_settings", v)}
            />
          </div>
        </ScrollArea>
      </TabsContent>
    </Tabs>
  );
}

