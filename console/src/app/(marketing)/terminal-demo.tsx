"use client";

import { useState } from "react";
import { cn } from "@/lib/utils";
import { Globe, Network, Layers, Search } from "lucide-react";

const tabs = [
  {
    id: "scrape",
    label: "Scrape",
    icon: Globe,
    comment: "# Scrape any URL to clean markdown",
    endpoint: "/scrape",
    body: '{"url": "https://example.com", "formats": ["markdown"]}',
    response: (
      <>
        <Line>{`{`}</Line>
        <Line indent>
          <Key>markdown</Key>
          <Punct>: </Punct>
          <Str>&quot;# Example Domain\nThis domain is for use in illustrative...&quot;</Str>
          <Punct>,</Punct>
        </Line>
        <Line indent>
          <Key>metadata</Key>
          <Punct>{`: { `}</Punct>
          <Key>title</Key>
          <Punct>: </Punct>
          <Str>&quot;Example Domain&quot;</Str>
          <Punct>{`, `}</Punct>
          <Key>language</Key>
          <Punct>: </Punct>
          <Str>&quot;en&quot;</Str>
          <Punct>{` },`}</Punct>
        </Line>
        <Line indent>
          <Key>credits_used</Key>
          <Punct>: </Punct>
          <Num>1</Num>
        </Line>
        <Line>{`}`}</Line>
      </>
    ),
  },
  {
    id: "map",
    label: "Map",
    icon: Network,
    comment: "# Discover all URLs on a website",
    endpoint: "/map",
    body: '{"url": "https://docs.example.com"}',
    response: (
      <>
        <Line>{`{`}</Line>
        <Line indent>
          <Key>total_urls</Key>
          <Punct>: </Punct>
          <Num>847</Num>
          <Punct>,</Punct>
        </Line>
        <Line indent>
          <Key>urls</Key>
          <Punct>{`: [`}</Punct>
        </Line>
        <Line indent={2}>
          <Str>&quot;https://docs.example.com/getting-started&quot;</Str>
          <Punct>,</Punct>
        </Line>
        <Line indent={2}>
          <Str>&quot;https://docs.example.com/api-reference&quot;</Str>
          <Punct>,</Punct>
        </Line>
        <Line indent={2}>
          <Str>&quot;https://docs.example.com/guides/auth&quot;</Str>
          <Punct>,</Punct>
        </Line>
        <Line indent={2}>
          <Punct>...</Punct>
        </Line>
        <Line indent>
          <Punct>{`],`}</Punct>
        </Line>
        <Line indent>
          <Key>credits_used</Key>
          <Punct>: </Punct>
          <Num>1</Num>
        </Line>
        <Line>{`}`}</Line>
      </>
    ),
  },
  {
    id: "crawl",
    label: "Crawl",
    icon: Layers,
    comment: "# Crawl a site and index to Meilisearch",
    endpoint: "/crawl",
    body: '{"url": "https://docs.example.com", "max_pages": 500, "index_uid": "docs"}',
    response: (
      <>
        <Line>{`{`}</Line>
        <Line indent>
          <Key>job_id</Key>
          <Punct>: </Punct>
          <Str>&quot;crawl_8f2a4b1c&quot;</Str>
          <Punct>,</Punct>
        </Line>
        <Line indent>
          <Key>status</Key>
          <Punct>: </Punct>
          <Str>&quot;running&quot;</Str>
          <Punct>,</Punct>
        </Line>
        <Line indent>
          <Key>pages_crawled</Key>
          <Punct>: </Punct>
          <Num>0</Num>
          <Punct>,</Punct>
        </Line>
        <Line indent>
          <Key>max_pages</Key>
          <Punct>: </Punct>
          <Num>500</Num>
          <Punct>,</Punct>
        </Line>
        <Line indent>
          <Key>index_uid</Key>
          <Punct>: </Punct>
          <Str>&quot;docs&quot;</Str>
        </Line>
        <Line>{`}`}</Line>
      </>
    ),
  },
  {
    id: "search",
    label: "Search",
    icon: Search,
    comment: "# Search any website instantly",
    endpoint: "/search",
    body: '{"url": "https://docs.example.com", "q": "getting started"}',
    response: (
      <>
        <Line>{`{`}</Line>
        <Line indent>
          <Key>hits</Key>
          <Punct>{`: [`}</Punct>
        </Line>
        <Line indent={2}>
          <Punct>{`{ `}</Punct>
          <Key>title</Key>
          <Punct>: </Punct>
          <Str>&quot;Getting Started Guide&quot;</Str>
          <Punct>, </Punct>
          <Key>url</Key>
          <Punct>: </Punct>
          <Str>&quot;https://docs.example.com/getting-started&quot;</Str>
          <Punct>{` },`}</Punct>
        </Line>
        <Line indent={2}>
          <Punct>{`{ `}</Punct>
          <Key>title</Key>
          <Punct>: </Punct>
          <Str>&quot;Quick Start Tutorial&quot;</Str>
          <Punct>, </Punct>
          <Key>url</Key>
          <Punct>: </Punct>
          <Str>&quot;https://docs.example.com/quick-start&quot;</Str>
          <Punct>{` },`}</Punct>
        </Line>
        <Line indent={2}>
          <Punct>...</Punct>
        </Line>
        <Line indent>
          <Punct>{`],`}</Punct>
        </Line>
        <Line indent>
          <Key>estimatedTotalHits</Key>
          <Punct>: </Punct>
          <Num>23</Num>
          <Punct>,</Punct>
        </Line>
        <Line indent>
          <Key>processingTimeMs</Key>
          <Punct>: </Punct>
          <Num>4</Num>
        </Line>
        <Line>{`}`}</Line>
      </>
    ),
  },
];

function Line({
  children,
  indent,
}: {
  children: React.ReactNode;
  indent?: boolean | number;
}) {
  const level = indent === true ? 1 : typeof indent === "number" ? indent : 0;
  return (
    <div style={{ paddingLeft: `${level * 1}rem` }} className="text-zinc-400">
      {children}
    </div>
  );
}

function Key({ children }: { children: React.ReactNode }) {
  return <span className="text-indigo-400">{children}</span>;
}
function Str({ children }: { children: React.ReactNode }) {
  return <span className="text-emerald-400">{children}</span>;
}
function Num({ children }: { children: React.ReactNode }) {
  return <span className="text-cyan-400">{children}</span>;
}
function Punct({ children }: { children: React.ReactNode }) {
  return <span className="text-zinc-600">{children}</span>;
}

export function TerminalDemo() {
  const [activeTab, setActiveTab] = useState("scrape");
  const tab = tabs.find((t) => t.id === activeTab)!;

  return (
    <div className="mx-auto mt-20 max-w-3xl">
      <div className="relative">
        {/* Glow behind terminal */}
        <div className="absolute -inset-4 rounded-3xl bg-gradient-to-r from-indigo-500/20 via-cyan-500/10 to-indigo-500/20 blur-2xl opacity-60" />
        <div className="relative rounded-2xl border border-white/10 bg-zinc-900 shadow-2xl overflow-hidden">
          {/* Tab bar */}
          <div className="flex items-center border-b border-white/5 bg-zinc-900/80">
            <div className="flex gap-1.5 pl-4 pr-3 py-3">
              <div className="h-3 w-3 rounded-full bg-zinc-700" />
              <div className="h-3 w-3 rounded-full bg-zinc-700" />
              <div className="h-3 w-3 rounded-full bg-zinc-700" />
            </div>
            <div className="flex">
              {tabs.map((t) => (
                <button
                  key={t.id}
                  onClick={() => setActiveTab(t.id)}
                  className={cn(
                    "flex items-center gap-1.5 px-4 py-3 text-xs font-medium transition-colors border-b-2 -mb-[1px]",
                    activeTab === t.id
                      ? "text-white border-indigo-400"
                      : "text-zinc-500 border-transparent hover:text-zinc-300"
                  )}
                >
                  <t.icon className="h-3 w-3" />
                  {t.label}
                </button>
              ))}
            </div>
          </div>

          {/* Terminal content */}
          <div className="p-6 font-mono text-[13px] leading-relaxed space-y-4">
            <div>
              <p className="text-zinc-600">{tab.comment}</p>
              <div className="mt-1">
                <span className="text-emerald-400">$</span>{" "}
                <span className="text-zinc-300">
                  curl -X POST https://scrapix.meilisearch.dev{tab.endpoint}
                </span>{" "}
                <span className="text-zinc-600">\</span>
              </div>
              <div className="pl-4 text-zinc-300">
                -H{" "}
                <span className="text-amber-300">
                  &quot;Authorization: Bearer sk_live_...&quot;
                </span>{" "}
                <span className="text-zinc-600">\</span>
              </div>
              <div className="pl-4 text-zinc-300">
                -d{" "}
                <span className="text-amber-300">
                  &apos;{tab.body}&apos;
                </span>
              </div>
            </div>
            <div className="border-t border-white/5 pt-4">
              <p className="text-zinc-600 mb-2"># Response</p>
              {tab.response}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
