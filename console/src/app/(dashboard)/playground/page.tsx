"use client";

import { useState, useEffect } from "react";
import { createClient } from "@/lib/supabase/client";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Loader2, Play, Copy, ExternalLink } from "lucide-react";
import { toast } from "sonner";
import { submitScrape, createCrawl } from "@/lib/api";

interface ApiKey {
  id: string;
  name: string;
  prefix: string;
}

export default function PlaygroundPage() {
  const router = useRouter();
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [selectedKey, setSelectedKey] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [result, setResult] = useState<string>("");
  const [activeTab, setActiveTab] = useState("scrape");
  const [lastJobId, setLastJobId] = useState<string | null>(null);

  // Scrape form state
  const [scrapeUrl, setScrapeUrl] = useState("https://example.com");
  const [scrapeFormat, setScrapeFormat] = useState("markdown");

  // Crawl form state
  const [crawlUrls, setCrawlUrls] = useState("https://example.com");
  const [crawlDepth, setCrawlDepth] = useState("2");
  const [crawlMaxPages, setCrawlMaxPages] = useState("100");

  const supabase = createClient();

  useEffect(() => {
    fetchKeys();
  }, []);

  const fetchKeys = async () => {
    const {
      data: { user },
    } = await supabase.auth.getUser();
    if (!user) return;

    const { data: membership } = await supabase
      .from("account_members")
      .select("account_id")
      .eq("user_id", user.id)
      .single();

    if (membership) {
      const { data: keysData } = await supabase
        .from("api_keys")
        .select("id, name, prefix")
        .eq("account_id", membership.account_id)
        .eq("active", true)
        .order("created_at", { ascending: false });

      if (keysData && keysData.length > 0) {
        setKeys(keysData);
        setSelectedKey(keysData[0].id);
      }
    }
  };

  const handleScrape = async () => {
    if (!scrapeUrl.trim()) {
      toast.error("Please enter a URL");
      return;
    }

    setLoading(true);
    setResult("");
    setLastJobId(null);

    try {
      const data = await submitScrape(scrapeUrl, scrapeFormat);
      setResult(JSON.stringify(data, null, 2));
    } catch (error) {
      setResult(
        JSON.stringify(
          {
            error:
              error instanceof Error
                ? error.message
                : "Failed to fetch. Is the API running?",
          },
          null,
          2
        )
      );
    }

    setLoading(false);
  };

  const handleCrawl = async () => {
    const urls = crawlUrls
      .split("\n")
      .map((u) => u.trim())
      .filter((u) => u);
    if (urls.length === 0) {
      toast.error("Please enter at least one URL");
      return;
    }

    setLoading(true);
    setResult("");
    setLastJobId(null);

    try {
      const data = await createCrawl({
        start_urls: urls,
        max_depth: parseInt(crawlDepth),
        max_pages: parseInt(crawlMaxPages),
        index_uid: `playground-${Date.now()}`,
      });
      setResult(JSON.stringify(data, null, 2));
      if (data.job_id) {
        setLastJobId(data.job_id);
      }
    } catch (error) {
      setResult(
        JSON.stringify(
          {
            error:
              error instanceof Error
                ? error.message
                : "Failed to fetch. Is the API running?",
          },
          null,
          2
        )
      );
    }

    setLoading(false);
  };

  const copyResult = () => {
    navigator.clipboard.writeText(result);
    toast.success("Copied to clipboard");
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Playground</h2>
        <p className="text-muted-foreground">
          Test the Scrapix API directly from your browser
        </p>
      </div>

      {keys.length === 0 ? (
        <Card>
          <CardContent className="py-8 text-center">
            <p className="text-muted-foreground">
              You need an API key to use the playground.{" "}
              <a href="/api-keys" className="text-primary hover:underline">
                Create one here
              </a>
              .
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid gap-6 lg:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Request</CardTitle>
              <CardDescription>Configure and send your request</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-2">
                <Label>API Key</Label>
                <Select value={selectedKey} onValueChange={setSelectedKey}>
                  <SelectTrigger>
                    <SelectValue placeholder="Select an API key" />
                  </SelectTrigger>
                  <SelectContent>
                    {keys.map((key) => (
                      <SelectItem key={key.id} value={key.id}>
                        {key.name} ({key.prefix})
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              <Tabs value={activeTab} onValueChange={setActiveTab}>
                <TabsList className="grid w-full grid-cols-2">
                  <TabsTrigger value="scrape">Scrape</TabsTrigger>
                  <TabsTrigger value="crawl">Crawl</TabsTrigger>
                </TabsList>

                <TabsContent value="scrape" className="space-y-4">
                  <div className="space-y-2">
                    <Label htmlFor="scrapeUrl">URL</Label>
                    <Input
                      id="scrapeUrl"
                      type="url"
                      placeholder="https://example.com"
                      value={scrapeUrl}
                      onChange={(e) => setScrapeUrl(e.target.value)}
                    />
                  </div>
                  <div className="space-y-2">
                    <Label>Output Format</Label>
                    <Select value={scrapeFormat} onValueChange={setScrapeFormat}>
                      <SelectTrigger>
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="markdown">Markdown</SelectItem>
                        <SelectItem value="html">HTML</SelectItem>
                        <SelectItem value="text">Plain Text</SelectItem>
                        <SelectItem value="json">Structured JSON</SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                  <Button
                    className="w-full"
                    onClick={handleScrape}
                    disabled={loading}
                  >
                    {loading ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <Play className="mr-2 h-4 w-4" />
                    )}
                    Scrape URL
                  </Button>
                </TabsContent>

                <TabsContent value="crawl" className="space-y-4">
                  <div className="space-y-2">
                    <Label htmlFor="crawlUrls">Start URLs (one per line)</Label>
                    <Textarea
                      id="crawlUrls"
                      placeholder="https://example.com&#10;https://example.com/docs"
                      value={crawlUrls}
                      onChange={(e) => setCrawlUrls(e.target.value)}
                      rows={3}
                    />
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div className="space-y-2">
                      <Label htmlFor="crawlDepth">Max Depth</Label>
                      <Input
                        id="crawlDepth"
                        type="number"
                        min="1"
                        max="10"
                        value={crawlDepth}
                        onChange={(e) => setCrawlDepth(e.target.value)}
                      />
                    </div>
                    <div className="space-y-2">
                      <Label htmlFor="crawlMaxPages">Max Pages</Label>
                      <Input
                        id="crawlMaxPages"
                        type="number"
                        min="1"
                        max="10000"
                        value={crawlMaxPages}
                        onChange={(e) => setCrawlMaxPages(e.target.value)}
                      />
                    </div>
                  </div>
                  <Button
                    className="w-full"
                    onClick={handleCrawl}
                    disabled={loading}
                  >
                    {loading ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : (
                      <Play className="mr-2 h-4 w-4" />
                    )}
                    Start Crawl
                  </Button>
                </TabsContent>
              </Tabs>
            </CardContent>
          </Card>

          <Card>
            <CardHeader className="flex flex-row items-center justify-between space-y-0">
              <div>
                <CardTitle>Response</CardTitle>
                <CardDescription>API response will appear here</CardDescription>
              </div>
              {result && (
                <Button variant="ghost" size="icon" onClick={copyResult}>
                  <Copy className="h-4 w-4" />
                </Button>
              )}
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="h-[400px] bg-muted rounded-lg p-4 overflow-auto">
                {loading ? (
                  <div className="flex items-center justify-center h-full">
                    <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
                  </div>
                ) : result ? (
                  <pre className="text-sm whitespace-pre-wrap">{result}</pre>
                ) : (
                  <p className="text-muted-foreground text-center">
                    Send a request to see the response
                  </p>
                )}
              </div>

              {lastJobId && (
                <Button
                  className="w-full"
                  variant="outline"
                  onClick={() => router.push(`/jobs/${lastJobId}`)}
                >
                  <ExternalLink className="mr-2 h-4 w-4" />
                  View Job
                </Button>
              )}
            </CardContent>
          </Card>
        </div>
      )}
    </div>
  );
}
