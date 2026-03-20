"use client";

import { useState, useMemo } from "react";
import Image from "next/image";
import { useApiKeys } from "@/lib/hooks";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Check,
  Copy,
  Terminal,
  Globe,
  Bot,
  ChevronDown,
  ChevronRight,
  ExternalLink,
  Zap,
} from "lucide-react";
import { toast } from "sonner";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";

const API_URL = "https://scrapix.meilisearch.dev";
const MCP_URL = `${API_URL}/mcp`;

function buildCursorDeepLink(): string {
  const config = JSON.stringify({ url: MCP_URL });
  const encoded = btoa(config);
  return `cursor://anysphere.cursor-deeplink/mcp/install?name=Scrapix&config=${encoded}`;
}

function CopyButton({ text, label }: { text: string; label: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    toast.success("Copied to clipboard");
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <Button variant="outline" size="sm" onClick={handleCopy} className="gap-2">
      {copied ? (
        <Check className="h-3.5 w-3.5 text-green-500" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
      {label}
    </Button>
  );
}

function CodeBlock({ code, language = "json" }: { code: string; language?: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(code);
    setCopied(true);
    toast.success("Copied to clipboard");
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="relative group">
      <pre className="rounded-lg border bg-muted/50 p-4 text-sm font-mono overflow-x-auto">
        <code>{code}</code>
      </pre>
      <Button
        variant="ghost"
        size="icon"
        className="absolute top-2 right-2 h-8 w-8 opacity-0 group-hover:opacity-100 transition-opacity"
        onClick={handleCopy}
      >
        {copied ? (
          <Check className="h-4 w-4 text-green-500" />
        ) : (
          <Copy className="h-4 w-4" />
        )}
      </Button>
    </div>
  );
}

const toolGroups = [
  {
    name: "Web Operations",
    tools: [
      { name: "scrape_url", description: "Scrape a single URL with optional JS rendering and AI enrichment" },
      { name: "map_url", description: "Discover all URLs on a website with metadata" },
      { name: "search_url", description: "Search indexed documents for a domain" },
      { name: "create_crawl", description: "Start an async crawl job" },
      { name: "create_crawl_sync", description: "Start a synchronous crawl (waits for completion)" },
      { name: "create_crawl_bulk", description: "Batch-create multiple crawl jobs" },
    ],
  },
  {
    name: "Job Management",
    tools: [
      { name: "job_status", description: "Get current status of a crawl job" },
      { name: "list_jobs", description: "List all jobs with pagination" },
      { name: "cancel_job", description: "Cancel a running or queued job" },
    ],
  },
  {
    name: "Configuration",
    tools: [
      { name: "create_config", description: "Save a named crawl configuration" },
      { name: "list_configs", description: "List saved configurations" },
      { name: "trigger_config", description: "Start a crawl from a saved config" },
    ],
  },
  {
    name: "Engine Management",
    tools: [
      { name: "create_engine", description: "Register a Meilisearch instance" },
      { name: "list_engines", description: "List registered engines" },
      { name: "set_default_engine", description: "Set the default engine for your account" },
      { name: "search_engine_index", description: "Search a specific index on an engine" },
    ],
  },
  {
    name: "Diagnostics",
    tools: [
      { name: "health", description: "Server health check" },
      { name: "handle_stats", description: "System-wide statistics" },
      { name: "handle_errors", description: "Recent error records" },
      { name: "handle_domains", description: "Per-domain performance metrics" },
    ],
  },
];

export default function McpPage() {
  const { data: keys = [], isLoading } = useApiKeys();
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>({});

  const activeKey = keys.find((k) => k.active);
  const keyPlaceholder = activeKey ? `${activeKey.prefix}...` : "sk_live_...";

  const cursorDeepLink = useMemo(() => buildCursorDeepLink(), []);
  const claudeCodeCommand = `claude mcp add --transport http scrapix ${MCP_URL}`;

  const claudeCodeConfig = `{
  "mcpServers": {
    "scrapix": {
      "command": "scrapix-mcp",
      "args": [
        "--api-url", "${API_URL}",
        "--api-key", "${keyPlaceholder}"
      ]
    }
  }
}`;

  const claudeCodeEnvConfig = `{
  "mcpServers": {
    "scrapix": {
      "command": "scrapix-mcp",
      "env": {
        "SCRAPIX_API_URL": "${API_URL}",
        "SCRAPIX_API_KEY": "${keyPlaceholder}"
      }
    }
  }
}`;

  const cursorConfig = `{
  "mcpServers": {
    "scrapix": {
      "command": "scrapix-mcp",
      "args": [
        "--api-url", "${API_URL}",
        "--api-key", "${keyPlaceholder}"
      ]
    }
  }
}`;

  const toggleGroup = (name: string) => {
    setOpenGroups((prev) => ({ ...prev, [name]: !prev[name] }));
  };

  if (isLoading) {
    return (
      <div className="space-y-6">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-4 w-96" />
        <Skeleton className="h-64 w-full" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">MCP Integration</h2>
        <p className="text-muted-foreground">
          Connect AI assistants like Claude to Scrapix via the Model Context Protocol
        </p>
      </div>

      {/* Quick Connect */}
      <Card className="border-primary/20 bg-primary/[0.02]">
        <CardHeader>
          <div className="flex items-center gap-2">
            <Zap className="h-5 w-5 text-primary" />
            <CardTitle>Quick Connect</CardTitle>
          </div>
          <CardDescription>
            One-click installation for supported clients. Uses the remote HTTP endpoint with OAuth — no API key needed.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-4 sm:grid-cols-3">
            {/* Cursor — deep link */}
            <a
              href={cursorDeepLink}
              className="flex items-center gap-3 rounded-lg border bg-card p-4 transition-colors hover:bg-muted/50"
            >
              <Image src="/cursor-icon.svg" alt="Cursor" width={32} height={32} className="h-8 w-8 shrink-0 dark:invert" />
              <div className="min-w-0">
                <p className="text-sm font-medium">Add to Cursor</p>
                <p className="text-xs text-muted-foreground truncate">Opens Cursor with MCP pre-configured</p>
              </div>
            </a>

            {/* Claude App — instructions */}
            <div className="flex items-center gap-3 rounded-lg border bg-card p-4">
              <Image src="/claude-icon.svg" alt="Claude" width={32} height={32} className="h-8 w-8 shrink-0" />
              <div className="min-w-0 space-y-1.5">
                <p className="text-sm font-medium">Claude App</p>
                <p className="text-xs text-muted-foreground">
                  Settings &gt; Connectors &gt; Add Integration
                </p>
                <CopyButton text={MCP_URL} label="Copy URL" />
              </div>
            </div>

            {/* Claude Code — CLI command */}
            <div className="flex items-center gap-3 rounded-lg border bg-card p-4">
              <Terminal className="h-8 w-8 shrink-0 text-muted-foreground" />
              <div className="min-w-0 space-y-1.5">
                <p className="text-sm font-medium">Claude Code</p>
                <p className="text-xs text-muted-foreground">
                  Run in your terminal
                </p>
                <CopyButton text={claudeCodeCommand} label="Copy command" />
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Standalone setup */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Terminal className="h-5 w-5 text-muted-foreground" />
            <CardTitle>Standalone Server</CardTitle>
            <Badge variant="outline" className="ml-auto">stdio</Badge>
          </div>
          <CardDescription>
            For Claude Code, Cursor, and local IDE integrations. The <code className="text-xs bg-muted px-1 py-0.5 rounded">scrapix-mcp</code> binary
            connects to the Scrapix API and exposes all endpoints as MCP tools.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <h4 className="text-sm font-medium">Claude Code</h4>
              <CopyButton text={claudeCodeCommand} label="Copy CLI command" />
            </div>
            <p className="text-sm text-muted-foreground">
              Or add to your Claude Code MCP configuration manually:
            </p>
            <CodeBlock code={claudeCodeConfig} />
          </div>

          <div className="space-y-2">
            <h4 className="text-sm font-medium">Claude Code (environment variables)</h4>
            <CodeBlock code={claudeCodeEnvConfig} />
          </div>

          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <h4 className="text-sm font-medium">Cursor</h4>
              <Button variant="outline" size="sm" asChild className="gap-2">
                <a href={cursorDeepLink}>
                  <ExternalLink className="h-3.5 w-3.5" />
                  Add to Cursor
                </a>
              </Button>
            </div>
            <p className="text-sm text-muted-foreground">
              Or add to <code className="text-xs bg-muted px-1 py-0.5 rounded">.cursor/mcp.json</code> manually:
            </p>
            <CodeBlock code={cursorConfig} />
          </div>

          {!activeKey && (
            <div className="rounded-lg border border-yellow-500/20 bg-yellow-500/5 p-3">
              <p className="text-sm text-yellow-600 dark:text-yellow-400">
                No active API key found. Create one on the{" "}
                <a href="/dashboard/api-keys" className="underline font-medium">
                  API Keys
                </a>{" "}
                page to use in the configuration above.
              </p>
            </div>
          )}

          <div className="space-y-2">
            <h4 className="text-sm font-medium">Install from source</h4>
            <CodeBlock
              code="cargo build --bin scrapix-mcp --release"
              language="bash"
            />
          </div>
        </CardContent>
      </Card>

      {/* HTTP MCP endpoint */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Globe className="h-5 w-5 text-muted-foreground" />
            <CardTitle>HTTP Endpoint</CardTitle>
            <Badge variant="outline" className="ml-auto">OAuth 2.1</Badge>
          </div>
          <CardDescription>
            For Claude App and remote MCP clients that support OAuth discovery.
            No manual configuration required.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <h4 className="text-sm font-medium">Endpoint URL</h4>
            <CodeBlock code={`${API_URL}/mcp`} />
          </div>

          <div className="space-y-3">
            <h4 className="text-sm font-medium">How it works</h4>
            <ol className="list-decimal list-inside space-y-2 text-sm text-muted-foreground">
              <li>Point your MCP client to <code className="text-xs bg-muted px-1 py-0.5 rounded">{API_URL}/mcp</code></li>
              <li>The client discovers OAuth endpoints automatically via <code className="text-xs bg-muted px-1 py-0.5 rounded">/.well-known/oauth-authorization-server</code></li>
              <li>You&apos;ll be prompted to log in and authorize the client</li>
              <li>Tokens are refreshed automatically — no manual key management</li>
            </ol>
          </div>
        </CardContent>
      </Card>

      {/* Available tools */}
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Bot className="h-5 w-5 text-muted-foreground" />
            <CardTitle>Available Tools</CardTitle>
            <Badge variant="secondary" className="ml-auto">
              {toolGroups.reduce((sum, g) => sum + g.tools.length, 0)} tools
            </Badge>
          </div>
          <CardDescription>
            All Scrapix API endpoints are exposed as MCP tools, auto-generated from the{" "}
            <a
              href={`${API_URL}/docs`}
              target="_blank"
              rel="noopener noreferrer"
              className="underline inline-flex items-center gap-1"
            >
              OpenAPI spec
              <ExternalLink className="h-3 w-3" />
            </a>
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {toolGroups.map((group) => (
            <Collapsible
              key={group.name}
              open={openGroups[group.name]}
              onOpenChange={() => toggleGroup(group.name)}
            >
              <CollapsibleTrigger className="flex w-full items-center justify-between rounded-lg border px-4 py-3 text-sm font-medium hover:bg-muted/50 transition-colors">
                <span className="flex items-center gap-2">
                  {group.name}
                  <Badge variant="secondary" className="text-xs">
                    {group.tools.length}
                  </Badge>
                </span>
                {openGroups[group.name] ? (
                  <ChevronDown className="h-4 w-4 text-muted-foreground" />
                ) : (
                  <ChevronRight className="h-4 w-4 text-muted-foreground" />
                )}
              </CollapsibleTrigger>
              <CollapsibleContent className="px-4 pb-2">
                <div className="mt-2 space-y-1">
                  {group.tools.map((tool) => (
                    <div
                      key={tool.name}
                      className="flex items-baseline gap-3 py-1.5 text-sm"
                    >
                      <code className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-xs font-mono">
                        {tool.name}
                      </code>
                      <span className="text-muted-foreground">
                        {tool.description}
                      </span>
                    </div>
                  ))}
                </div>
              </CollapsibleContent>
            </Collapsible>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
