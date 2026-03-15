import Link from "next/link";
import type { Metadata } from "next";
import { Button } from "@/components/ui/button";
import {
  ArrowRight,
  Check,
  Network,
  Globe,
  FileText,
  Zap,
} from "lucide-react";

export const metadata: Metadata = {
  title: "Map API",
  description:
    "Discover all URLs on a website with a single API call. Sitemaps, links, and resources — mapped instantly.",
};

export default function MapPage() {
  return (
    <>
      {/* Hero */}
      <section className="relative overflow-hidden">
        <div className="absolute inset-0">
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[600px] h-[400px] bg-cyan-500/10 rounded-full blur-[120px]" />
        </div>
        <div className="relative mx-auto max-w-6xl px-6 pt-24 pb-16 md:pt-32 md:pb-20">
          <div className="mx-auto max-w-3xl text-center">
            <div className="mb-6 inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-4 py-1.5 text-sm text-zinc-300">
              <Network className="h-3.5 w-3.5 text-cyan-400" />
              /map
            </div>
            <h1 className="mb-6 text-4xl font-bold tracking-tight md:text-5xl text-white">
              Discover every URL on{" "}
              <span className="bg-gradient-to-r from-cyan-400 to-indigo-400 bg-clip-text text-transparent" style={{ fontFamily: "var(--font-rubik-glitch), var(--font-geist-sans), sans-serif" }}>
                any website
              </span>
            </h1>
            <p className="mx-auto mb-10 max-w-xl text-lg text-zinc-400 leading-relaxed">
              Map the complete link structure of a website in seconds. Find all
              pages, sitemaps, and resources without fetching full content.
              Flat rate — 2 credits per call.
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
              <div className="absolute -inset-4 rounded-3xl bg-gradient-to-r from-cyan-500/20 via-indigo-500/10 to-cyan-500/20 blur-2xl opacity-40" />
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
                        curl -X POST https://scrapix.meilisearch.dev/map
                      </span>{" "}
                      <span className="text-zinc-600">\</span>
                    </div>
                    <div className="pl-4 text-zinc-300">
                      -H <span className="text-amber-300">&quot;Authorization: Bearer sk_live_...&quot;</span>{" "}
                      <span className="text-zinc-600">\</span>
                    </div>
                    <div className="pl-4 text-zinc-300">
                      -d <span className="text-amber-300">&apos;{`{"url": "https://docs.example.com"}`}&apos;</span>
                    </div>
                  </div>
                  <div className="border-t border-white/5 pt-4">
                    <p className="text-zinc-600 mb-2"># Response</p>
                    <div className="text-zinc-400">
                      <span className="text-zinc-600">{`{`}</span>
                      <br />
                      <span className="pl-4 inline-block">
                        <span className="text-indigo-400">&quot;total_urls&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-cyan-400">847</span>
                        <span className="text-zinc-600">,</span>
                      </span>
                      <br />
                      <span className="pl-4 inline-block">
                        <span className="text-indigo-400">&quot;urls&quot;</span>
                        <span className="text-zinc-600">{`: [`}</span>
                      </span>
                      <br />
                      <span className="pl-8 inline-block">
                        <span className="text-emerald-400">&quot;https://docs.example.com/getting-started&quot;</span>
                        <span className="text-zinc-600">,</span>
                      </span>
                      <br />
                      <span className="pl-8 inline-block">
                        <span className="text-emerald-400">&quot;https://docs.example.com/api-reference&quot;</span>
                        <span className="text-zinc-600">,</span>
                      </span>
                      <br />
                      <span className="pl-8 inline-block text-zinc-600">...</span>
                      <br />
                      <span className="pl-4 inline-block text-zinc-600">{`],`}</span>
                      <br />
                      <span className="pl-4 inline-block">
                        <span className="text-indigo-400">&quot;credits_used&quot;</span>
                        <span className="text-zinc-600">: </span>
                        <span className="text-cyan-400">2</span>
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

      {/* Use cases */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-2 text-center text-2xl font-bold text-white">
            Use cases
          </h2>
          <p className="mb-12 text-center text-zinc-400">
            Map is the fastest way to understand a website before crawling it.
          </p>

          <div className="mx-auto grid max-w-4xl gap-6 md:grid-cols-3">
            {[
              {
                icon: Network,
                title: "Pre-crawl discovery",
                description:
                  "Discover how many pages a site has and plan your crawl budget before spending credits on full content extraction.",
              },
              {
                icon: FileText,
                title: "Sitemap generation",
                description:
                  "Build a complete URL list for any site — even those without a sitemap.xml. Export and use in your own tools.",
              },
              {
                icon: Globe,
                title: "Link analysis",
                description:
                  "Understand the link structure and information architecture of any website at a glance.",
              },
            ].map(({ icon: Icon, title, description }) => (
              <div
                key={title}
                className="rounded-2xl border border-white/5 bg-white/[0.02] p-6"
              >
                <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-cyan-500/20 to-indigo-500/20 ring-1 ring-white/10">
                  <Icon className="h-5 w-5 text-cyan-400" />
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

      {/* Features */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <div className="mx-auto max-w-3xl">
            <h2 className="mb-12 text-center text-2xl font-bold text-white">
              How it works
            </h2>
            <div className="grid gap-x-12 gap-y-4 sm:grid-cols-2">
              {[
                "Crawls links and sitemaps recursively",
                "Returns a flat list of all discovered URLs",
                "Respects robots.txt directives",
                "Flat rate: 2 credits per call regardless of URLs found",
                "Configurable scope and depth",
                "Fast — no full content fetching",
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

      {/* CTA */}
      <section className="border-t border-white/5">
        <div className="relative mx-auto max-w-6xl px-6 py-24">
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="w-[600px] h-[300px] bg-cyan-500/10 rounded-full blur-[100px]" />
          </div>
          <div className="relative mx-auto max-w-2xl text-center">
            <h2 className="mb-4 text-3xl font-bold tracking-tight text-white">
              Map any website for 2 credits
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
