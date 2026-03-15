import Link from "next/link";
import Image from "next/image";
import { cookies } from "next/headers";
import { Button } from "@/components/ui/button";
import { Globe, Layers, Network, Search } from "lucide-react";

export default async function MarketingLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  const cookieStore = await cookies();
  const isLoggedIn = cookieStore.has("scrapix_session");
  return (
    <div className="min-h-screen flex flex-col bg-zinc-950 text-zinc-100">
      <header className="sticky top-0 z-50 border-b border-white/5 bg-zinc-950/80 backdrop-blur-xl">
        <div className="mx-auto flex h-16 max-w-6xl items-center justify-between px-6">
          <Link href="/">
            <Image
              src="/logotype_dark.svg"
              alt="Scrapix"
              width={120}
              height={32}
              className="h-6 w-auto"
            />
          </Link>
          <nav className="hidden md:flex items-center gap-8">
            <div className="group relative">
              <button className="text-sm text-zinc-400 hover:text-white transition-colors flex items-center gap-1">
                Products
                <svg className="h-3 w-3 transition-transform group-hover:rotate-180" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
                  <path strokeLinecap="round" strokeLinejoin="round" d="M19 9l-7 7-7-7" />
                </svg>
              </button>
              <div className="invisible group-hover:visible opacity-0 group-hover:opacity-100 transition-all duration-150 absolute top-full left-1/2 -translate-x-1/2 pt-2">
                <div className="w-64 rounded-xl border border-white/10 bg-zinc-900 p-2 shadow-2xl shadow-black/50">
                  <Link
                    href="/products/scrape"
                    className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm text-zinc-300 hover:bg-white/5 hover:text-white transition-colors"
                  >
                    <Globe className="h-4 w-4 text-indigo-400" />
                    <div>
                      <div className="font-medium">Scrape</div>
                      <div className="text-xs text-zinc-500">Extract data from any URL</div>
                    </div>
                  </Link>
                  <Link
                    href="/products/map"
                    className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm text-zinc-300 hover:bg-white/5 hover:text-white transition-colors"
                  >
                    <Network className="h-4 w-4 text-cyan-400" />
                    <div>
                      <div className="font-medium">Map</div>
                      <div className="text-xs text-zinc-500">Discover every URL on a site</div>
                    </div>
                  </Link>
                  <Link
                    href="/products/crawl"
                    className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm text-zinc-300 hover:bg-white/5 hover:text-white transition-colors"
                  >
                    <Layers className="h-4 w-4 text-violet-400" />
                    <div>
                      <div className="font-medium">Crawl</div>
                      <div className="text-xs text-zinc-500">Crawl sites and index everything</div>
                    </div>
                  </Link>
                  <Link
                    href="/products/search"
                    className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm text-zinc-300 hover:bg-white/5 hover:text-white transition-colors"
                  >
                    <Search className="h-4 w-4 text-emerald-400" />
                    <div>
                      <div className="font-medium">Search</div>
                      <div className="text-xs text-zinc-500">Search any website instantly</div>
                    </div>
                  </Link>
                </div>
              </div>
            </div>
            <Link
              href="/pricing"
              className="text-sm text-zinc-400 hover:text-white transition-colors"
            >
              Pricing
            </Link>
          </nav>
          <div className="flex items-center gap-3">
            {isLoggedIn ? (
              <Button
                size="sm"
                className="bg-white text-zinc-950 hover:bg-zinc-200"
                asChild
              >
                <Link href="/dashboard">Console</Link>
              </Button>
            ) : (
              <>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-zinc-400 hover:text-white hover:bg-white/5"
                  asChild
                >
                  <Link href="/login">Sign in</Link>
                </Button>
                <Button
                  size="sm"
                  className="bg-white text-zinc-950 hover:bg-zinc-200"
                  asChild
                >
                  <Link href="/signup">Get started</Link>
                </Button>
              </>
            )}
          </div>
        </div>
      </header>
      <main className="flex-1">{children}</main>
      <footer className="border-t border-white/5">
        <div className="mx-auto max-w-6xl px-6 py-16">
          <div className="grid gap-8 md:grid-cols-4">
            <div className="space-y-4">
              <Link href="/">
                <Image
                  src="/logotype_dark.svg"
                  alt="Scrapix"
                  width={100}
                  height={26}
                  className="h-5 w-auto"
                />
              </Link>
              <p className="text-sm text-zinc-500 leading-relaxed">
                High-performance web crawling and search indexing at internet
                scale.
              </p>
            </div>
            <div className="space-y-3">
              <h4 className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
                Product
              </h4>
              <ul className="space-y-2 text-sm text-zinc-500">
                <li>
                  <Link
                    href="/products/scrape"
                    className="hover:text-white transition-colors"
                  >
                    Scrape API
                  </Link>
                </li>
                <li>
                  <Link
                    href="/products/map"
                    className="hover:text-white transition-colors"
                  >
                    Map API
                  </Link>
                </li>
                <li>
                  <Link
                    href="/products/crawl"
                    className="hover:text-white transition-colors"
                  >
                    Crawl API
                  </Link>
                </li>
                <li>
                  <Link
                    href="/products/search"
                    className="hover:text-white transition-colors"
                  >
                    Search API
                  </Link>
                </li>
                <li>
                  <Link
                    href="/pricing"
                    className="hover:text-white transition-colors"
                  >
                    Pricing
                  </Link>
                </li>
                <li>
                  <Link
                    href="/login"
                    className="hover:text-white transition-colors"
                  >
                    Console
                  </Link>
                </li>
              </ul>
            </div>
            <div className="space-y-3">
              <h4 className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
                Developers
              </h4>
              <ul className="space-y-2 text-sm text-zinc-500">
                <li>
                  <span className="hover:text-white transition-colors cursor-default">
                    Documentation
                  </span>
                </li>
                <li>
                  <span className="hover:text-white transition-colors cursor-default">
                    API Reference
                  </span>
                </li>
              </ul>
            </div>
            <div className="space-y-3">
              <h4 className="text-xs font-semibold uppercase tracking-wider text-zinc-400">
                Company
              </h4>
              <ul className="space-y-2 text-sm text-zinc-500">
                <li>
                  <Link
                    href="/terms"
                    className="hover:text-white transition-colors"
                  >
                    Terms of Service
                  </Link>
                </li>
                <li>
                  <Link
                    href="/privacy"
                    className="hover:text-white transition-colors"
                  >
                    Privacy Policy
                  </Link>
                </li>
              </ul>
            </div>
          </div>
          <div className="mt-12 border-t border-white/5 pt-8 text-center text-sm text-zinc-600">
            Powered by{" "}
            <span className="text-zinc-400">Meilisearch</span>
          </div>
        </div>
      </footer>
    </div>
  );
}
