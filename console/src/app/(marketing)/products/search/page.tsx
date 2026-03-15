import Link from "next/link";
import type { Metadata } from "next";
import { Button } from "@/components/ui/button";
import {
  ArrowRight,
  Check,
  Search,
  Globe,
  FileText,
  Brain,
  Zap,
  Shield,
} from "lucide-react";

export const metadata: Metadata = {
  title: "Search API",
  description:
    "Search any website instantly. Crawl, index, and query web content with a single API call.",
};

export default function SearchPage() {
  return (
    <>
      {/* Hero */}
      <section className="relative overflow-hidden">
        <div className="absolute inset-0">
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[600px] h-[400px] bg-emerald-500/10 rounded-full blur-[120px]" />
        </div>
        <div className="relative mx-auto max-w-6xl px-6 pt-24 pb-16 md:pt-32 md:pb-20">
          <div className="mx-auto max-w-3xl text-center">
            <div className="mb-6 inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-4 py-1.5 text-sm text-zinc-300">
              <Search className="h-3.5 w-3.5 text-emerald-400" />
              /search
            </div>
            <h1 className="mb-6 text-4xl font-bold tracking-tight md:text-5xl text-white">
              Search any{" "}
              <span className="bg-gradient-to-r from-emerald-400 to-cyan-400 bg-clip-text text-transparent" style={{ fontFamily: "var(--font-rubik-glitch), var(--font-geist-sans), sans-serif" }}>
                website
              </span>
            </h1>
            <p className="mx-auto mb-10 max-w-xl text-lg text-zinc-400 leading-relaxed">
              Send a URL and a query, get back relevant results. Scrapix crawls,
              indexes, and searches the site for you — all in one API call.
            </p>
            <div className="flex flex-col items-center gap-4 sm:flex-row sm:justify-center">
              <Button
                size="lg"
                className="bg-white text-zinc-950 hover:bg-zinc-200 h-12 px-8 text-base"
                asChild
              >
                <Link href="/signup">
                  Try it free
                  <ArrowRight className="ml-2 h-4 w-4" />
                </Link>
              </Button>
              <Button
                variant="outline"
                size="lg"
                className="border-white/10 bg-white/5 text-white hover:bg-white/10 h-12 px-8 text-base"
                asChild
              >
                <Link href="/pricing">See pricing</Link>
              </Button>
            </div>
          </div>
        </div>
      </section>

      {/* Code example */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <div className="mx-auto max-w-3xl">
            <div className="relative">
              <div className="absolute -inset-4 rounded-3xl bg-gradient-to-r from-emerald-500/20 via-cyan-500/10 to-emerald-500/20 blur-2xl opacity-40" />
              <div className="relative rounded-2xl border border-white/10 bg-zinc-900 overflow-hidden">
                <div className="flex items-center border-b border-white/5 bg-zinc-900/80 px-4 py-3">
                  <div className="flex gap-1.5">
                    <div className="h-3 w-3 rounded-full bg-zinc-700" />
                    <div className="h-3 w-3 rounded-full bg-zinc-700" />
                    <div className="h-3 w-3 rounded-full bg-zinc-700" />
                  </div>
                </div>
                <div className="p-6 font-mono text-[13px] leading-relaxed space-y-4">
                  <div>
                    <div>
                      <span className="text-emerald-400">$</span>{" "}
                      <span className="text-zinc-300">
                        curl -X POST https://scrapix.meilisearch.dev/search
                      </span>{" "}
                      <span className="text-zinc-600">\</span>
                    </div>
                    <div className="pl-4 text-zinc-300">
                      -H <span className="text-amber-300">&quot;Authorization: Bearer sk_live_...&quot;</span>{" "}
                      <span className="text-zinc-600">\</span>
                    </div>
                    <div className="pl-4 text-zinc-300">
                      -d <span className="text-amber-300">&apos;{`{
    "url": "https://docs.example.com",
    "q": "getting started",
    "limit": 10
  }`}&apos;</span>
                    </div>
                  </div>
                  <div className="border-t border-white/5 pt-4">
                    <p className="text-zinc-600 mb-2"># Response — search results</p>
                    <div className="text-zinc-400">
                      <span className="text-zinc-600">{`{`}</span>
                      <br />
                      <span className="pl-4 inline-block">
                        <span className="text-indigo-400">&quot;hits&quot;</span>
                        <span className="text-zinc-600">{`: [`}</span>
                      </span>
                      <br />
                      <span className="pl-8 inline-block">
                        <span className="text-zinc-600">{`{ `}</span>
                        <span className="text-indigo-400">&quot;title&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-emerald-400">&quot;Getting Started Guide&quot;</span>
                        <span className="text-zinc-600">, </span>
                        <span className="text-indigo-400">&quot;url&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-emerald-400">&quot;/getting-started&quot;</span>
                        <span className="text-zinc-600">{` },`}</span>
                      </span>
                      <br />
                      <span className="pl-8 inline-block">
                        <span className="text-zinc-600">{`{ `}</span>
                        <span className="text-indigo-400">&quot;title&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-emerald-400">&quot;Quick Start Tutorial&quot;</span>
                        <span className="text-zinc-600">, </span>
                        <span className="text-indigo-400">&quot;url&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-emerald-400">&quot;/quick-start&quot;</span>
                        <span className="text-zinc-600">{` },`}</span>
                      </span>
                      <br />
                      <span className="pl-8 inline-block text-zinc-600">...</span>
                      <br />
                      <span className="pl-4 inline-block text-zinc-600">{`],`}</span>
                      <br />
                      <span className="pl-4 inline-block">
                        <span className="text-indigo-400">&quot;estimatedTotalHits&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-cyan-400">23</span>
                        <span className="text-zinc-600">,</span>
                      </span>
                      <br />
                      <span className="pl-4 inline-block">
                        <span className="text-indigo-400">&quot;processingTimeMs&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-cyan-400">4</span>
                      </span>
                      <br />
                      <span className="text-zinc-600">{`}`}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* How it works */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-2 text-center text-2xl font-bold text-white">
            From URL to search results
          </h2>
          <p className="mb-12 text-center text-zinc-400">
            Three steps, one API call. Scrapix handles the rest.
          </p>

          <div className="mx-auto grid max-w-5xl gap-6 md:grid-cols-3">
            {[
              {
                step: "01",
                title: "Crawl",
                description: "Scrapix crawls the target website, following links and respecting robots.txt.",
              },
              {
                step: "02",
                title: "Index",
                description: "Content is extracted, cleaned, and indexed into Meilisearch for full-text search.",
              },
              {
                step: "03",
                title: "Search",
                description: "Your query runs against the indexed content. Results come back ranked by relevance.",
              },
            ].map(({ step, title, description }) => (
              <div
                key={step}
                className="relative rounded-2xl border border-white/5 bg-white/[0.02] p-6"
              >
                <span
                  className="text-3xl font-bold bg-gradient-to-br from-emerald-400 to-cyan-400 bg-clip-text text-transparent"
                  style={{ fontFamily: "var(--font-rubik-glitch), var(--font-geist-sans), sans-serif" }}
                >
                  {step}
                </span>
                <h3 className="mt-3 mb-2 font-semibold text-white">{title}</h3>
                <p className="text-sm text-zinc-400 leading-relaxed">
                  {description}
                </p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Features grid */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-12 text-center text-2xl font-bold text-white">
            Built for developers
          </h2>
          <div className="mx-auto grid max-w-4xl gap-6 md:grid-cols-2 lg:grid-cols-3">
            {[
              {
                icon: Brain,
                title: "RAG pipelines",
                description: "Feed relevant web content into your LLM. Search docs and use results as context for AI answers.",
              },
              {
                icon: Search,
                title: "Site search",
                description: "Add search to any website without building an index yourself. Point Scrapix at a site and start querying.",
              },
              {
                icon: Zap,
                title: "Instant results",
                description: "Typo-tolerant, relevance-ranked results powered by Meilisearch. Responses in single-digit milliseconds.",
              },
              {
                icon: FileText,
                title: "Documentation search",
                description: "Index technical docs and search across multiple sources. Great for internal tooling and developer portals.",
              },
              {
                icon: Shield,
                title: "Polite crawling",
                description: "Automatic rate limiting per domain, robots.txt compliance, and configurable delays.",
              },
              {
                icon: Globe,
                title: "JS rendering",
                description: "Optionally render pages with a headless browser for JavaScript-heavy sites.",
              },
            ].map(({ icon: Icon, title, description }) => (
              <div
                key={title}
                className="rounded-2xl border border-white/5 bg-white/[0.02] p-6"
              >
                <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-emerald-500/20 to-cyan-500/20 ring-1 ring-white/10">
                  <Icon className="h-5 w-5 text-emerald-400" />
                </div>
                <h3 className="mb-2 font-semibold text-white">{title}</h3>
                <p className="text-sm text-zinc-400 leading-relaxed">
                  {description}
                </p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Checklist */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <div className="mx-auto max-w-3xl">
            <h2 className="mb-12 text-center text-2xl font-bold text-white">
              Powered by Meilisearch
            </h2>
            <div className="grid gap-x-12 gap-y-4 sm:grid-cols-2">
              {[
                "Typo-tolerant full-text search",
                "Relevant results ranked by quality",
                "Highlighted matching snippets",
                "Automatic crawling and indexing",
                "Results cached for fast repeat queries",
                "Works with any public website",
              ].map((feature) => (
                <div
                  key={feature}
                  className="flex items-start gap-3 text-sm"
                >
                  <Check className="h-4 w-4 mt-0.5 text-emerald-400 shrink-0" />
                  <span className="text-zinc-300">{feature}</span>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      {/* Pricing summary */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <div className="mx-auto max-w-2xl">
            <h2 className="mb-8 text-center text-2xl font-bold text-white">
              Search pricing
            </h2>
            <div className="rounded-2xl border border-white/5 bg-white/[0.02] p-6 space-y-3 text-sm">
              {[
                { label: "Search query (per request)", value: "1 cr" },
                { label: "Initial crawl (per page)", value: "1 cr" },
                { label: "JS rendering (per page)", value: "2 cr" },
                { label: "Subsequent searches (cached index)", value: "1 cr" },
                { label: "Search indexing", value: "Free", free: true },
              ].map(({ label, value, free }) => (
                <div key={label} className="flex justify-between">
                  <span className="text-zinc-400">{label}</span>
                  <span
                    className={`font-mono ${free ? "text-emerald-400" : "text-white"}`}
                  >
                    {value}
                  </span>
                </div>
              ))}
            </div>
            <p className="mt-4 text-center text-sm text-zinc-600">
              <Link href="/pricing" className="text-zinc-400 hover:text-white transition-colors underline underline-offset-4">
                View full pricing details
              </Link>
            </p>
          </div>
        </div>
      </section>

      {/* CTA */}
      <section className="border-t border-white/5">
        <div className="relative mx-auto max-w-6xl px-6 py-24">
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="w-[600px] h-[300px] bg-emerald-500/10 rounded-full blur-[100px]" />
          </div>
          <div className="relative mx-auto max-w-2xl text-center">
            <h2 className="mb-4 text-3xl font-bold tracking-tight text-white">
              Start searching in minutes
            </h2>
            <p className="mb-8 text-lg text-zinc-400">
              1,000 free credits. No credit card required.
            </p>
            <Button
              size="lg"
              className="bg-white text-zinc-950 hover:bg-zinc-200 h-12 px-8 text-base"
              asChild
            >
              <Link href="/signup">
                Get started for free
                <ArrowRight className="ml-2 h-4 w-4" />
              </Link>
            </Button>
          </div>
        </div>
      </section>
    </>
  );
}
