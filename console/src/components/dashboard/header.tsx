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
  "/dashboard": "Dashboard",
  "/dashboard/scrape": "Scrape",
  "/dashboard/crawl": "Crawl",
  "/dashboard/map": "Map",
  "/dashboard/jobs": "Jobs",
  "/dashboard/configs": "Configs",
  "/dashboard/engines": "Engines",
  "/dashboard/api-keys": "API Keys",
  "/dashboard/billing": "Billing",
  "/dashboard/usage": "Usage",
  "/dashboard/settings": "Settings",
};

function getPageTitle(pathname: string): string {
  if (pageTitles[pathname]) return pageTitles[pathname];
  if (pathname.startsWith("/dashboard/jobs/")) return "Job Details";
  if (pathname.startsWith("/dashboard/configs/")) return "Config Details";
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
