import Link from "next/link";
import { Button } from "@/components/ui/button";
import {
  Globe,
  Zap,
  Search,
  Code,
  ArrowRight,
  FileText,
  Layers,
  Brain,
  Shield,
  BarChart3,
  Network,
  Terminal,
  Check,
} from "lucide-react";
import { TerminalDemo } from "./terminal-demo";

function FeatureCard({
  icon: Icon,
  title,
  description,
}: {
  icon: React.ComponentType<{ className?: string }>;
  title: string;
  description: string;
}) {
  return (
    <div className="group relative rounded-2xl border border-white/5 bg-white/[0.02] p-6 transition-all hover:border-white/10 hover:bg-white/[0.04]">
      <div className="mb-4 flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-indigo-500/20 to-cyan-500/20 ring-1 ring-white/10">
        <Icon className="h-5 w-5 text-indigo-400" />
      </div>
      <h3 className="mb-2 font-semibold text-white">{title}</h3>
      <p className="text-sm text-zinc-400 leading-relaxed">{description}</p>
    </div>
  );
}

export default function HomePage() {
  return (
    <>
      {/* Hero */}
      <section className="relative overflow-hidden">
        {/* Background grid + glow */}
        <div className="absolute inset-0">
          <div
            className="absolute inset-0 opacity-[0.03]"
            style={{
              backgroundImage:
                "linear-gradient(rgba(255,255,255,0.1) 1px, transparent 1px), linear-gradient(90deg, rgba(255,255,255,0.1) 1px, transparent 1px)",
              backgroundSize: "64px 64px",
            }}
          />
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[800px] h-[600px] bg-indigo-500/15 rounded-full blur-[120px]" />
          <div className="absolute top-20 left-1/3 w-[400px] h-[400px] bg-cyan-500/10 rounded-full blur-[100px]" />
          <div className="absolute top-40 right-1/4 w-[300px] h-[300px] bg-rose-500/8 rounded-full blur-[80px]" />
        </div>

        <div className="relative mx-auto max-w-6xl px-6 pt-24 pb-16 md:pt-32 md:pb-24 lg:pt-40 lg:pb-32">
          <div className="mx-auto max-w-3xl text-center">
            <div className="mb-8 inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-4 py-1.5 text-sm text-zinc-300 backdrop-blur-sm">
              <Zap className="h-3.5 w-3.5 text-amber-400" />
              Now in public beta
            </div>
            <h1 className="mb-6 text-5xl font-bold tracking-tight md:text-6xl lg:text-7xl">
              <span className="bg-gradient-to-b from-white to-zinc-400 bg-clip-text text-transparent">
                Turn any website
              </span>
              <br />
              <span className="bg-gradient-to-r from-indigo-400 via-cyan-400 to-indigo-400 bg-clip-text text-transparent">
                into{" "}
                <span style={{ fontFamily: "var(--font-rubik-glitch), var(--font-geist-sans), sans-serif" }}>
                  structured
                </span>{" "}
                data
              </span>
            </h1>
            <p className="mx-auto mb-10 max-w-xl text-lg text-zinc-400 leading-relaxed">
              Scrape, crawl, and index the web with a single API call. Clean
              markdown, metadata, and AI-powered extractions — in seconds.
            </p>
            <div className="flex flex-col items-center gap-4 sm:flex-row sm:justify-center">
              <Button
                size="lg"
                className="bg-white text-zinc-950 hover:bg-zinc-200 h-12 px-8 text-base"
                asChild
              >
                <Link href="/signup">
                  Start for free
                  <ArrowRight className="ml-2 h-4 w-4" />
                </Link>
              </Button>
              <Button
                variant="outline"
                size="lg"
                className="border-white/10 bg-white/5 text-white hover:bg-white/10 h-12 px-8 text-base"
                asChild
              >
                <Link href="#features">
                  <Terminal className="mr-2 h-4 w-4" />
                  See how it works
                </Link>
              </Button>
            </div>
          </div>

          {/* Code preview */}
          <TerminalDemo />
        </div>
      </section>

      {/* Logos / trust strip */}
      <section className="border-y border-white/5 bg-white/[0.01]">
        <div className="mx-auto max-w-6xl px-6 py-10">
          <div className="flex flex-wrap items-center justify-center gap-x-12 gap-y-4 text-zinc-600 text-sm">
            <span>Trusted by teams building with</span>
            <span className="font-semibold text-zinc-400">Meilisearch</span>
            <span className="text-zinc-800">|</span>
            <span className="font-mono text-xs text-zinc-500">
              500K+ pages crawled in beta
            </span>
          </div>
        </div>
      </section>

      {/* Features */}
      <section id="features" className="relative">
        <div className="absolute top-0 left-0 w-[500px] h-[500px] bg-indigo-500/5 rounded-full blur-[120px]" />
        <div className="relative mx-auto max-w-6xl px-6 py-28">
          <div className="mx-auto mb-4 max-w-2xl text-center">
            <p className="text-sm font-medium text-indigo-400 mb-3 uppercase tracking-widest">
              Capabilities
            </p>
            <h2 className="mb-4 text-3xl font-bold tracking-tight md:text-4xl text-white">
              Everything you need to extract web data
            </h2>
            <p className="text-zinc-400 text-lg">
              From single-page scraping to full-site crawling — one API,
              unlimited possibilities.
            </p>
          </div>

          {/* Bento-ish grid */}
          <div className="mt-16 grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            <FeatureCard
              icon={Globe}
              title="Scrape any page"
              description="Extract clean markdown, HTML, metadata, and structured data from any URL. Handles SPAs and JavaScript-rendered content."
            />
            <FeatureCard
              icon={Layers}
              title="Crawl entire sites"
              description="Recursively crawl websites with configurable depth, domain restrictions, and page limits. Monitor progress in real-time."
            />
            <FeatureCard
              icon={Network}
              title="Discover URLs with Map"
              description="Map out the link structure of any website. Discover all pages, sitemaps, and resources without full content fetching."
            />
            <FeatureCard
              icon={Search}
              title="Instant search indexing"
              description="Crawled content is automatically indexed into Meilisearch engines for lightning-fast, typo-tolerant full-text search."
            />
            <FeatureCard
              icon={Brain}
              title="AI-powered extraction"
              description="Use LLMs to extract structured data, generate summaries, and answer questions about any page content."
            />
            <FeatureCard
              icon={FileText}
              title="Multiple output formats"
              description="Get results as markdown, raw HTML, metadata, JSON-LD schemas, content blocks, or custom CSS selector extractions."
            />
          </div>

          {/* Secondary features as a simpler list */}
          <div className="mt-12 grid gap-6 md:grid-cols-3">
            {[
              { icon: Code, text: "Developer-first RESTful API with SDKs and playground" },
              { icon: Shield, text: "Automatic robots.txt compliance and polite rate limiting" },
              { icon: BarChart3, text: "Built-in analytics dashboards for every request" },
            ].map(({ icon: Icon, text }) => (
              <div key={text} className="flex items-start gap-3 text-sm">
                <Icon className="h-4 w-4 mt-0.5 text-zinc-500 shrink-0" />
                <span className="text-zinc-400">{text}</span>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* How it works */}
      <section id="how-it-works" className="relative border-t border-white/5">
        <div className="relative mx-auto max-w-6xl px-6 py-28">
          <div className="mx-auto mb-16 max-w-2xl text-center">
            <p className="text-sm font-medium text-indigo-400 mb-3 uppercase tracking-widest">
              Quick start
            </p>
            <h2 className="mb-4 text-3xl font-bold tracking-tight md:text-4xl text-white">
              Three steps to structured data
            </h2>
          </div>

          <div className="grid gap-8 md:grid-cols-3">
            {[
              {
                step: "01",
                title: "Create an API key",
                description:
                  "Sign up and generate an API key from the console. Your first 1,000 credits are free.",
              },
              {
                step: "02",
                title: "Send a request",
                description:
                  "Call /scrape for single pages, /crawl for full sites, or /map to discover URLs.",
              },
              {
                step: "03",
                title: "Get structured data",
                description:
                  "Receive clean markdown, metadata, and search-ready content. Auto-index into Meilisearch.",
              },
            ].map(({ step, title, description }) => (
              <div
                key={step}
                className="relative rounded-2xl border border-white/5 bg-white/[0.02] p-8"
              >
                <span
                  className="text-5xl font-bold bg-gradient-to-br from-indigo-400 to-cyan-400 bg-clip-text text-transparent"
                  style={{ fontFamily: "var(--font-rubik-glitch), var(--font-geist-sans), sans-serif" }}
                >
                  {step}
                </span>
                <h3 className="mt-4 mb-2 font-semibold text-white">{title}</h3>
                <p className="text-sm text-zinc-400 leading-relaxed">
                  {description}
                </p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* Pricing */}
      <section id="pricing" className="relative border-t border-white/5">
        <div className="absolute top-0 right-0 w-[500px] h-[500px] bg-cyan-500/5 rounded-full blur-[120px]" />
        <div className="relative mx-auto max-w-6xl px-6 py-28">
          <div className="mx-auto mb-16 max-w-2xl text-center">
            <p className="text-sm font-medium text-indigo-400 mb-3 uppercase tracking-widest">
              Pricing
            </p>
            <h2 className="mb-4 text-3xl font-bold tracking-tight md:text-4xl text-white">
              Simple, credit-based pricing
            </h2>
            <p className="text-zinc-400 text-lg">
              Pay only for what you use. No monthly minimums, no surprise fees.
            </p>
          </div>

          <div className="mx-auto grid max-w-4xl gap-6 md:grid-cols-3">
            {[
              {
                name: "Scrape",
                credits: "1",
                detail: "per page scraped",
                extras: ["+1 per feature format", "+5 for AI extraction"],
                highlighted: false,
              },
              {
                name: "Crawl",
                credits: "1",
                detail: "per page crawled",
                extras: [
                  "2 cr/page with JS rendering",
                  "Auto-indexing included",
                ],
                highlighted: true,
              },
              {
                name: "Map",
                credits: "2",
                detail: "per request",
                extras: ["Flat rate per call", "Link structure mapping"],
                highlighted: false,
              },
            ].map(({ name, credits, detail, extras, highlighted }) => (
              <div
                key={name}
                className={`relative rounded-2xl p-8 ${
                  highlighted
                    ? "border-2 border-indigo-500/50 bg-gradient-to-b from-indigo-500/10 to-transparent"
                    : "border border-white/5 bg-white/[0.02]"
                }`}
              >
                {highlighted && (
                  <div className="absolute -top-3 left-1/2 -translate-x-1/2 rounded-full bg-indigo-500 px-3 py-0.5 text-xs font-medium text-white">
                    Most popular
                  </div>
                )}
                <h3 className="mb-1 text-lg font-semibold text-white">
                  {name}
                </h3>
                <div className="mb-4">
                  <span className="text-4xl font-bold text-white">
                    {credits}
                  </span>{" "}
                  <span className="text-sm text-zinc-500">credits</span>
                </div>
                <p className="mb-6 text-sm text-zinc-500">{detail}</p>
                <ul className="space-y-2">
                  {extras.map((extra) => (
                    <li
                      key={extra}
                      className="flex items-center gap-2 text-sm text-zinc-400"
                    >
                      <Check className="h-3.5 w-3.5 text-indigo-400" />
                      {extra}
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>

          <div className="mt-12 flex flex-col items-center gap-4 sm:flex-row sm:justify-center">
            <Button
              size="lg"
              className="bg-white text-zinc-950 hover:bg-zinc-200 h-12 px-8 text-base"
              asChild
            >
              <Link href="/signup">
                Start with 1,000 free credits
                <ArrowRight className="ml-2 h-4 w-4" />
              </Link>
            </Button>
            <Button
              variant="outline"
              size="lg"
              className="border-white/10 bg-white/5 text-white hover:bg-white/10 h-12 px-8 text-base"
              asChild
            >
              <Link href="/pricing">See full pricing details</Link>
            </Button>
          </div>
        </div>
      </section>

      {/* CTA */}
      <section className="border-t border-white/5">
        <div className="relative mx-auto max-w-6xl px-6 py-28">
          <div className="absolute inset-0 flex items-center justify-center">
            <div className="w-[600px] h-[300px] bg-indigo-500/10 rounded-full blur-[100px]" />
          </div>
          <div className="relative mx-auto max-w-2xl text-center">
            <h2 className="mb-4 text-3xl font-bold tracking-tight md:text-4xl text-white">
              Ready to start scraping?
            </h2>
            <p className="mb-8 text-lg text-zinc-400">
              Create your free account and start extracting structured data from
              the web in minutes.
            </p>
            <div className="flex flex-col items-center gap-4 sm:flex-row sm:justify-center">
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
              <Button
                variant="outline"
                size="lg"
                className="border-white/10 bg-white/5 text-white hover:bg-white/10 h-12 px-8 text-base"
                asChild
              >
                <Link href="/login">Sign in to console</Link>
              </Button>
            </div>
          </div>
        </div>
      </section>
    </>
  );
}
