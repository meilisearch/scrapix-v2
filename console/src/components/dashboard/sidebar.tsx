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
  Moon,
  Sun,
  Zap,
} from "lucide-react";
import { logout } from "@/lib/auth";
import { useMe } from "@/lib/hooks";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

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
  const { resolvedTheme, theme, setTheme } = useTheme();
  const { data: user } = useMe();
  const [mounted, setMounted] = useState(false);
  const isDark = resolvedTheme === "dark" || resolvedTheme === "glitch";

  useEffect(() => {
    setMounted(true);
  }, []);

  const handleLogout = async () => {
    await logout();
    router.push("/login");
    router.refresh();
  };

  const initials =
    user?.full_name
      ?.split(" ")
      .map((n: string) => n[0])
      .join("")
      .toUpperCase() || user?.email?.[0].toUpperCase() || "U";

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center px-6 border-b">
        <Link href="/" className="flex items-center" onClick={onNavigate}>
          <Image
            src={isDark ? "/logotype_dark.svg" : "/logotype_light.svg"}
            alt="Scrapix"
            width={120}
            height={32}
            className="h-7 w-auto"
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
                    ? "bg-primary/10 text-primary glitch-nav-active"
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
            <p className="px-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground/60 glitch-section-label">
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
                      ? "bg-primary/10 text-primary glitch-nav-active"
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
      <Separator className="glitch-separator" />
      <div className="p-3 space-y-1">
        {mounted && (
          <Button
            variant="ghost"
            className="w-full justify-start text-muted-foreground"
            onClick={() => {
              if (theme === "light") setTheme("dark");
              else if (theme === "dark") setTheme("glitch");
              else setTheme("light");
            }}
          >
            {theme === "glitch" ? (
              <Zap className="mr-3 h-4 w-4" />
            ) : theme === "dark" ? (
              <Sun className="mr-3 h-4 w-4" />
            ) : (
              <Moon className="mr-3 h-4 w-4" />
            )}
            {theme === "glitch" ? "Glitch" : theme === "dark" ? "Light" : "Dark"} mode
          </Button>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              className="w-full justify-start text-muted-foreground"
            >
              <Avatar className="mr-3 h-5 w-5">
                <AvatarFallback className="text-[10px]">{initials}</AvatarFallback>
              </Avatar>
              <span className="truncate">{user?.full_name || user?.email || "Account"}</span>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="top" align="start" className="w-56">
            <DropdownMenuLabel>
              <div className="flex flex-col space-y-1">
                <p className="text-sm font-medium">
                  {user?.full_name || "User"}
                </p>
                <p className="text-xs text-muted-foreground">{user?.email}</p>
              </div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => { router.push("/settings"); onNavigate?.(); }}>
              Settings
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={handleLogout}>Sign out</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
  );
}

export function Sidebar() {
  return (
    <aside className="hidden md:flex h-full w-56 flex-col border-r bg-card glitch-sidebar">
      <SidebarContent />
    </aside>
  );
}
