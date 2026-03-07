"use client";

import { useState } from "react";
import { usePathname } from "next/navigation";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { Menu } from "lucide-react";
import { SidebarContent } from "./sidebar";

const pageTitles: Record<string, string> = {
  "/": "Dashboard",
  "/scrape": "Scrape",
  "/crawl": "Crawl",
  "/map": "Map",
  "/jobs": "Jobs",
  "/configs": "Configs",
  "/engines": "Engines",
  "/api-keys": "API Keys",
  "/billing": "Billing",
  "/usage": "Usage",
  "/settings": "Settings",
};

function getPageTitle(pathname: string): string {
  if (pageTitles[pathname]) return pageTitles[pathname];
  if (pathname.startsWith("/jobs/")) return "Job Details";
  if (pathname.startsWith("/configs/")) return "Config Details";
  return "Console";
}

export function MobileHeader() {
  const [mobileOpen, setMobileOpen] = useState(false);
  const pathname = usePathname();

  return (
    <header className="flex h-14 items-center gap-3 border-b px-4 md:hidden glitch-header">
      <Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
        <SheetTrigger asChild>
          <Button variant="ghost" size="icon">
            <Menu className="h-5 w-5" />
          </Button>
        </SheetTrigger>
        <SheetContent side="left" className="w-56 p-0">
          <SheetTitle className="sr-only">Navigation</SheetTitle>
          <SidebarContent onNavigate={() => setMobileOpen(false)} />
        </SheetContent>
      </Sheet>
      <h1 className="text-lg font-semibold">{getPageTitle(pathname)}</h1>
    </header>
  );
}
