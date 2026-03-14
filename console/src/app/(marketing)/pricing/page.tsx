import Link from "next/link";
import type { Metadata } from "next";
import { Button } from "@/components/ui/button";
import {
  ArrowRight,
  Check,
  Globe,
  Layers,
  Network,
  Brain,
  Code,
  Zap,
  HelpCircle,
} from "lucide-react";

export const metadata: Metadata = {
  title: "Pricing",
  description:
    "Simple, credit-based pricing. Pay only for what you use — no monthly minimums, no surprise fees.",
};

const creditPacks = [
  { amount: "1,000", price: 10, perCredit: "0.010", badge: null },
  { amount: "5,000", price: 40, perCredit: "0.008", badge: "Save 20%" },
  { amount: "10,000", price: 70, perCredit: "0.007", badge: "Save 30%" },
  { amount: "50,000", price: 250, perCredit: "0.005", badge: "Save 50%" },
];

const endpoints = [
  {
    name: "Scrape",
    icon: Globe,
    base: 1,
    description: "Extract content from a single URL",
    extras: [
      { label: "Base scrape", credits: 1 },
      { label: "+ each feature format (markdown, schema...)", credits: 1 },
      { label: "+ AI extraction", credits: 5 },
      { label: "+ AI summary", credits: 5 },
    ],
  },
  {
    name: "Map",
    icon: Network,
    base: 2,
    description: "Discover all URLs on a website",
    extras: [
      { label: "Flat rate per call", credits: 2 },
    ],
  },
  {
    name: "Crawl",
    icon: Layers,
    base: 1,
    description: "Crawl a site and index to Meilisearch",
    extras: [
      { label: "Per page (HTTP)", credits: 1 },
      { label: "Per page (JS rendering)", credits: 2 },
      { label: "+ each feature (metadata, schema...)", credits: 1 },
      { label: "+ AI extraction (per page)", credits: 5 },
      { label: "+ AI summary (per page)", credits: 5 },
      { label: "Search indexing included", credits: 0 },
    ],
  },
];

const faqs = [
  {
    q: "What are credits?",
    a: "Credits are the unit of usage in Scrapix. Each API call consumes a certain number of credits depending on the endpoint and features used. You purchase credits upfront and they never expire.",
  },
  {
    q: "Do credits expire?",
    a: "No. Credits never expire. Use them whenever you need, at your own pace.",
  },
  {
    q: "What happens when I run out of credits?",
    a: "API calls will return a 402 error. You can enable auto top-up in the console to automatically purchase more credits when your balance drops below a threshold.",
  },
  {
    q: "Is there a free tier?",
    a: "Every new account receives 1,000 free credits — enough to scrape about 1,000 pages or run a small crawl. No credit card required to sign up.",
  },
  {
    q: "Can I get volume discounts?",
    a: "Yes. The larger the credit pack you purchase, the lower the per-credit cost. For very high volume (100K+ credits/month), contact us for custom pricing.",
  },
  {
    q: "How does JS rendering work?",
    a: "When a page requires JavaScript to load content (SPAs, dynamic sites), enable the JS rendering option. It uses a headless browser and costs 1 additional credit per page.",
  },
];

export default function PricingPage() {
  return (
    <>
      {/* Hero */}
      <section className="relative overflow-hidden">
        <div className="absolute inset-0">
          <div className="absolute top-0 left-1/2 -translate-x-1/2 w-[600px] h-[400px] bg-indigo-500/10 rounded-full blur-[120px]" />
        </div>
        <div className="relative mx-auto max-w-6xl px-6 pt-24 pb-16 md:pt-32 md:pb-20">
          <div className="mx-auto max-w-2xl text-center">
            <p
              className="text-sm font-medium text-indigo-400 mb-3 uppercase tracking-widest"
            >
              Pricing
            </p>
            <h1 className="mb-4 text-4xl font-bold tracking-tight md:text-5xl text-white">
              Simple, transparent pricing
            </h1>
            <p className="text-lg text-zinc-400 leading-relaxed">
              Pay only for what you use. Buy credits upfront, use them anytime.
              No subscriptions, no surprise fees.
            </p>
          </div>
        </div>
      </section>

      {/* Credit packs */}
      <section className="relative border-t border-white/5">
        <div className="relative mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-2 text-center text-2xl font-bold text-white">
            Buy credits
          </h2>
          <p className="mb-12 text-center text-zinc-400">
            Larger packs = lower cost per credit. Credits never expire.
          </p>

          <div className="mx-auto grid max-w-5xl gap-4 sm:grid-cols-2 lg:grid-cols-4">
            {creditPacks.map(({ amount, price, perCredit, badge }) => (
              <div
                key={amount}
                className={`relative rounded-2xl p-6 transition-all ${
                  badge === "Save 50%"
                    ? "border-2 border-indigo-500/50 bg-gradient-to-b from-indigo-500/10 to-transparent"
                    : "border border-white/5 bg-white/[0.02] hover:border-white/10"
                }`}
              >
                {badge && (
                  <div
                    className={`absolute -top-2.5 right-4 rounded-full px-2.5 py-0.5 text-[11px] font-medium ${
                      badge === "Save 50%"
                        ? "bg-indigo-500 text-white"
                        : "bg-white/10 text-zinc-300"
                    }`}
                  >
                    {badge}
                  </div>
                )}
                <div className="mb-4">
                  <span className="text-3xl font-bold text-white">
                    {amount}
                  </span>
                  <span className="ml-1 text-sm text-zinc-500">credits</span>
                </div>
                <div className="mb-1 text-2xl font-semibold text-white">
                  ${price}
                </div>
                <p className="mb-6 text-xs text-zinc-500">
                  ${perCredit} per credit
                </p>
                <Button
                  className={`w-full ${
                    badge === "Save 50%"
                      ? "bg-white text-zinc-950 hover:bg-zinc-200"
                      : "bg-white/10 text-white hover:bg-white/15"
                  }`}
                  asChild
                >
                  <Link href="/signup">Get started</Link>
                </Button>
              </div>
            ))}
          </div>

          <p className="mt-8 text-center text-sm text-zinc-600">
            Every new account gets{" "}
            <span className="text-zinc-400">1,000 free credits</span> — no
            credit card required.
          </p>
        </div>
      </section>

      {/* Credit breakdown per endpoint */}
      <section className="relative border-t border-white/5">
        <div className="absolute top-0 left-0 w-[500px] h-[500px] bg-cyan-500/5 rounded-full blur-[120px]" />
        <div className="relative mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-2 text-center text-2xl font-bold text-white">
            How credits work
          </h2>
          <p className="mb-12 text-center text-zinc-400">
            Each endpoint has a base cost. Premium features add extra credits.
          </p>

          <div className="mx-auto grid max-w-5xl gap-6 md:grid-cols-3">
            {endpoints.map(({ name, icon: Icon, description, extras }) => (
              <div
                key={name}
                className="rounded-2xl border border-white/5 bg-white/[0.02] p-6"
              >
                <div className="mb-4 flex items-center gap-3">
                  <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-indigo-500/20 to-cyan-500/20 ring-1 ring-white/10">
                    <Icon className="h-5 w-5 text-indigo-400" />
                  </div>
                  <div>
                    <h3 className="font-semibold text-white">{name}</h3>
                    <p className="text-xs text-zinc-500">{description}</p>
                  </div>
                </div>
                <div className="space-y-3">
                  {extras.map(({ label, credits }) => (
                    <div
                      key={label}
                      className="flex items-center justify-between text-sm"
                    >
                      <span className="text-zinc-400">{label}</span>
                      <span
                        className={
                          credits === 0
                            ? "text-emerald-400 text-xs"
                            : "font-mono text-white"
                        }
                      >
                        {credits === 0 ? "Free" : `${credits} cr`}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>

          {/* Example calculation */}
          <div className="mx-auto mt-12 max-w-2xl rounded-2xl border border-white/5 bg-white/[0.02] p-6">
            <h3 className="mb-4 text-sm font-medium text-zinc-300 flex items-center gap-2">
              <Zap className="h-4 w-4 text-amber-400" />
              Example: Crawl 500 pages with JS rendering + metadata
            </h3>
            <div className="space-y-2 text-sm">
              <div className="flex justify-between">
                <span className="text-zinc-400">500 pages with JS rendering (2 cr/page)</span>
                <span className="font-mono text-white">1,000 cr</span>
              </div>
              <div className="flex justify-between">
                <span className="text-zinc-400">
                  Metadata extraction (1 cr/page)
                </span>
                <span className="font-mono text-white">500 cr</span>
              </div>
              <div className="flex justify-between">
                <span className="text-zinc-400">Search indexing</span>
                <span className="text-emerald-400">Free</span>
              </div>
              <div className="mt-3 flex justify-between border-t border-white/5 pt-3">
                <span className="font-medium text-white">Total</span>
                <span className="font-mono font-bold text-white">
                  1,500 cr
                </span>
              </div>
              <p className="text-xs text-zinc-600 pt-1">
                That&apos;s $15 with the starter pack, or $7.50 with the 50K pack.
              </p>
            </div>
          </div>
        </div>
      </section>

      {/* Feature comparison */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-2 text-center text-2xl font-bold text-white">
            Every account includes
          </h2>
          <p className="mb-12 text-center text-zinc-400">
            No tiers, no feature gates. Every feature is available to every user.
          </p>

          <div className="mx-auto grid max-w-3xl gap-x-12 gap-y-4 sm:grid-cols-2">
            {[
              "Unlimited API keys",
              "All output formats (markdown, HTML, metadata, JSON-LD)",
              "JavaScript rendering",
              "AI extraction & summarization",
              "Real-time crawl monitoring",
              "Meilisearch search indexing",
              "Usage analytics dashboard",
              "Auto top-up & spend limits",
              "robots.txt compliance",
              "Configurable rate limiting",
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
      </section>

      {/* FAQ */}
      <section className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-20">
          <h2 className="mb-12 text-center text-2xl font-bold text-white">
            Frequently asked questions
          </h2>

          <div className="mx-auto max-w-3xl divide-y divide-white/5">
            {faqs.map(({ q, a }) => (
              <div key={q} className="py-6 first:pt-0 last:pb-0">
                <h3 className="mb-2 font-medium text-white flex items-start gap-2">
                  <HelpCircle className="h-4 w-4 mt-0.5 text-indigo-400 shrink-0" />
                  {q}
                </h3>
                <p className="pl-6 text-sm text-zinc-400 leading-relaxed">
                  {a}
                </p>
              </div>
            ))}
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
            <h2 className="mb-4 text-3xl font-bold tracking-tight md:text-4xl text-white">
              Start with 1,000 free credits
            </h2>
            <p className="mb-8 text-lg text-zinc-400">
              No credit card required. Sign up and start scraping in minutes.
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
