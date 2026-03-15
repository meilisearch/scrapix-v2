import Link from "next/link";
import type { Metadata } from "next";
import { Button } from "@/components/ui/button";
import {
  ArrowRight,
  Check,
  Globe,
  FileText,
  Brain,
  Code,
  Zap,
} from "lucide-react";

export const metadata: Metadata = {
  title: "Scrape API",
  description:
    "Extract clean markdown, metadata, and structured data from any URL with a single API call.",
};

export default function ScrapePage() {
  return (
    <>
      {/* Hero */}
      <section className="relative overflow-hidden">
        <div className="absolute inset-0">
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[600px] h-[400px] bg-indigo-500/10 rounded-full blur-[120px]" />
        </div>
        <div className="relative mx-auto max-w-6xl px-6 pt-24 pb-16 md:pt-32 md:pb-20">
          <div className="mx-auto max-w-3xl text-center">
            <div className="mb-6 inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-4 py-1.5 text-sm text-zinc-300">
              <Globe className="h-3.5 w-3.5 text-indigo-400" />
              /scrape
            </div>
            <h1 className="mb-6 text-4xl font-bold tracking-tight md:text-5xl text-white">
              Extract data from{" "}
              <span className="bg-gradient-to-r from-indigo-400 to-cyan-400 bg-clip-text text-transparent" style={{ fontFamily: "var(--font-rubik-glitch), var(--font-geist-sans), sans-serif" }}>
                any URL
              </span>
            </h1>
            <p className="mx-auto mb-10 max-w-xl text-lg text-zinc-400 leading-relaxed">
              Send a URL, get back clean markdown, metadata, JSON-LD schemas,
              and AI-powered extractions. Handles JavaScript-rendered pages out
              of the box.
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
              <div className="absolute -inset-4 rounded-3xl bg-gradient-to-r from-indigo-500/20 via-cyan-500/10 to-indigo-500/20 blur-2xl opacity-40" />
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
                        curl -X POST https://scrapix.meilisearch.dev/scrape
                      </span>{" "}
                      <span className="text-zinc-600">\</span>
                    </div>
                    <div className="pl-4 text-zinc-300">
                      -H <span className="text-amber-300">&quot;Authorization: Bearer sk_live_...&quot;</span>{" "}
                      <span className="text-zinc-600">\</span>
                    </div>
                    <div className="pl-4 text-zinc-300">
                      -d <span className="text-amber-300">&apos;{`{
    "url": "https://example.com",
    "formats": ["markdown", "metadata", "schema"],
    "ai_summary": true
  }`}&apos;</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Output formats */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-2 text-center text-2xl font-bold text-white">
            Multiple output formats
          </h2>
          <p className="mb-12 text-center text-zinc-400">
            Choose exactly what you need. Combine multiple formats in a single request.
          </p>

          <div className="mx-auto grid max-w-5xl gap-4 md:grid-cols-2 lg:grid-cols-3">
            {[
              {
                icon: FileText,
                name: "Markdown",
                description:
                  "Clean, readable markdown with preserved heading structure, links, and formatting.",
                cost: "1 credit",
              },
              {
                icon: Code,
                name: "HTML",
                description:
                  "Raw or cleaned HTML. Great for custom parsing pipelines or archival.",
                cost: "1 credit",
              },
              {
                icon: Globe,
                name: "Metadata",
                description:
                  "Title, description, language, Open Graph tags, Twitter cards, and more.",
                cost: "1 credit",
              },
              {
                icon: Code,
                name: "JSON-LD / Schema",
                description:
                  "Structured data extracted from JSON-LD, microdata, and RDFa embedded in the page.",
                cost: "1 credit",
              },
              {
                icon: Brain,
                name: "AI Summary",
                description:
                  "LLM-generated summary of the page content. Concise and accurate.",
                cost: "5 credits",
              },
              {
                icon: Brain,
                name: "AI Extraction",
                description:
                  "Extract structured data using a custom prompt. Define your own schema.",
                cost: "5 credits",
              },
            ].map(({ icon: Icon, name, description, cost }) => (
              <div
                key={name}
                className="rounded-2xl border border-white/5 bg-white/[0.02] p-6"
              >
                <div className="mb-3 flex items-center justify-between">
                  <div className="flex items-center gap-2">
                    <Icon className="h-4 w-4 text-indigo-400" />
                    <h3 className="font-semibold text-white">{name}</h3>
                  </div>
                  <span
                    className={`text-xs font-mono ${cost === "Free" ? "text-emerald-400" : "text-zinc-500"}`}
                  >
                    {cost}
                  </span>
                </div>
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
              Built for reliability
            </h2>
            <div className="grid gap-x-12 gap-y-4 sm:grid-cols-2">
              {[
                "JavaScript rendering with headless browser",
                "Automatic retry on transient failures",
                "Configurable timeout and wait conditions",
                "Custom CSS selector extraction",
                "Content block splitting",
                "robots.txt compliance",
                "Proxy rotation support",
                "Custom headers and cookies",
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
            <div className="w-[600px] h-[300px] bg-indigo-500/10 rounded-full blur-[100px]" />
          </div>
          <div className="relative mx-auto max-w-2xl text-center">
            <h2 className="mb-4 text-3xl font-bold tracking-tight text-white">
              Start scraping in minutes
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
