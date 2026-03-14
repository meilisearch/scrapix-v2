import Image from "next/image";
import Link from "next/link";
import { ThemeToggle } from "./theme-toggle";

export default function AuthLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <div className="grid min-h-screen lg:grid-cols-2">
      <div className="relative hidden lg:flex flex-col bg-zinc-900 p-10 text-white dark:border-r">
        <div className="absolute inset-0 bg-zinc-900" />
        <div className="relative z-20 flex items-center gap-2">
          <Image
            src="/icon_dark_transparent.svg"
            alt="Scrapix"
            width={40}
            height={40}
            className="h-10 w-10"
          />
          <Image
            src="/logotype_dark.svg"
            alt="Scrapix"
            width={140}
            height={36}
            className="h-7 w-auto"
          />
        </div>
        <div className="relative z-20 mt-auto">
          <blockquote className="space-y-2">
            <p className="text-lg">
              High-performance web crawling and search indexing at internet scale.
            </p>
            <footer className="text-sm text-zinc-400">
              Powered by Meilisearch
            </footer>
          </blockquote>
        </div>
      </div>
      <div className="relative flex items-center justify-center p-8">
        <ThemeToggle />
        <div className="mx-auto flex w-full flex-col justify-center space-y-6 sm:w-[350px]">
          {children}
          <p className="px-8 text-center text-sm text-muted-foreground">
            By continuing, you agree to our{" "}
            <Link
              href="/terms"
              className="underline underline-offset-4 hover:text-primary"
            >
              Terms of Service
            </Link>{" "}
            and{" "}
            <Link
              href="/privacy"
              className="underline underline-offset-4 hover:text-primary"
            >
              Privacy Policy
            </Link>
            .
          </p>
        </div>
      </div>
    </div>
  );
}
