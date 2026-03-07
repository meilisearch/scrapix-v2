"use client";

import Link from "next/link";
import Image from "next/image";
import { usePathname } from "next/navigation";
import { useTheme } from "next-themes";
import { cn } from "@/lib/utils";
import {
  LayoutDashboard,
  Key,
  Globe,
  Layers,
  ListTodo,
  CreditCard,
  Settings,
  LogOut,
  FolderCog,
  Database,
  BarChart3,
  Network,
} from "lucide-react";
import { logout } from "@/lib/auth";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";

const topNav = [
  { name: "Dashboard", href: "/", icon: LayoutDashboard },
];

const navGroups = [
  {
    label: "Playground",
    items: [
      { name: "Scrape", href: "/scrape", icon: Globe },
      { name: "Map", href: "/map", icon: Network },
      { name: "Crawl", href: "/crawl", icon: Layers },
    ],
  },
  {
    label: "Management",
    items: [
      { name: "Jobs", href: "/jobs", icon: ListTodo },
      { name: "Configs", href: "/configs", icon: FolderCog },
      { name: "Engines", href: "/engines", icon: Database },
    ],
  },
  {
    label: "Account",
    items: [
      { name: "API Keys", href: "/api-keys", icon: Key },
      { name: "Billing", href: "/billing", icon: CreditCard },
      { name: "Usage", href: "/usage", icon: BarChart3 },
      { name: "Settings", href: "/settings", icon: Settings },
    ],
  },
];

export function SidebarContent({ onNavigate }: { onNavigate?: () => void }) {
  const pathname = usePathname();
  const router = useRouter();
  const { resolvedTheme } = useTheme();
  const isDark = resolvedTheme === "dark" || resolvedTheme === "neon";

  const handleLogout = async () => {
    await logout();
    router.push("/login");
    router.refresh();
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center px-6 border-b">
        <Link href="/" className="flex items-center space-x-2" onClick={onNavigate}>
          <Image
            src={isDark ? "/icon_dark_transparent.svg" : "/icon_light_transparent.svg"}
            alt="Scrapix"
            width={32}
            height={32}
            className="h-8 w-8"
          />
          <Image
            src={isDark ? "/logotype_dark.svg" : "/logotype_light.svg"}
            alt="Scrapix"
            width={100}
            height={28}
            className="h-5 w-auto"
          />
        </Link>
      </div>
      <nav className="flex-1 space-y-4 px-3 py-4">
        <div className="space-y-1">
          {topNav.map((item) => {
            const isActive = pathname === item.href;
            return (
              <Link
                key={item.name}
                href={item.href}
                onClick={onNavigate}
                className={cn(
                  "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
                  isActive
                    ? "bg-primary/10 text-primary neon-nav-active"
                    : "text-muted-foreground hover:bg-muted hover:text-foreground"
                )}
              >
                <item.icon className="h-4 w-4" />
                {item.name}
              </Link>
            );
          })}
        </div>
        {navGroups.map((group) => (
          <div key={group.label} className="space-y-1">
            <p className="px-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground/60 neon-section-label">
              {group.label}
            </p>
            {group.items.map((item) => {
              const isActive =
                pathname === item.href ||
                (item.href !== "/" && pathname.startsWith(item.href));
              return (
                <Link
                  key={item.name}
                  href={item.href}
                  onClick={onNavigate}
                  className={cn(
                    "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
                    isActive
                      ? "bg-primary/10 text-primary neon-nav-active"
                      : "text-muted-foreground hover:bg-muted hover:text-foreground"
                  )}
                >
                  <item.icon className="h-4 w-4" />
                  {item.name}
                </Link>
              );
            })}
          </div>
        ))}
      </nav>
      <Separator className="neon-separator" />
      <div className="p-3">
        <Button
          variant="ghost"
          className="w-full justify-start text-muted-foreground"
          onClick={handleLogout}
        >
          <LogOut className="mr-3 h-4 w-4" />
          Sign out
        </Button>
      </div>
    </div>
  );
}

export function Sidebar() {
  return (
    <aside className="hidden md:flex h-full w-56 flex-col border-r bg-card neon-sidebar">
      <SidebarContent />
    </aside>
  );
}
