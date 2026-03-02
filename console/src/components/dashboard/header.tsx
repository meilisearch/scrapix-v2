"use client";

import { useEffect, useState } from "react";
import { usePathname } from "next/navigation";
import { useTheme } from "next-themes";
import { getMe, logout, type AuthUser } from "@/lib/auth";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { useRouter } from "next/navigation";
import { Menu, Moon, Sun } from "lucide-react";
import { SidebarContent } from "./sidebar";

const pageTitles: Record<string, string> = {
  "/": "Dashboard",
  "/playground": "Playground",
  "/jobs": "Jobs",
  "/configs": "Configs",
  "/api-keys": "API Keys",
  "/billing": "Billing",
  "/settings": "Settings",
};

function getPageTitle(pathname: string): string {
  if (pageTitles[pathname]) return pageTitles[pathname];
  if (pathname.startsWith("/jobs/")) return "Job Details";
  if (pathname.startsWith("/configs/")) return "Config Details";
  return "Console";
}

export function Header() {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [mobileOpen, setMobileOpen] = useState(false);
  const router = useRouter();
  const pathname = usePathname();
  const { theme, setTheme } = useTheme();
  const [mounted, setMounted] = useState(false);

  useEffect(() => {
    setMounted(true);
    getMe().then(setUser).catch(() => {});
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
    <header className="flex h-14 items-center justify-between border-b px-4 md:px-6">
      <div className="flex items-center gap-3">
        <Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
          <SheetTrigger asChild>
            <Button variant="ghost" size="icon" className="md:hidden">
              <Menu className="h-5 w-5" />
            </Button>
          </SheetTrigger>
          <SheetContent side="left" className="w-56 p-0">
            <SheetTitle className="sr-only">Navigation</SheetTitle>
            <SidebarContent onNavigate={() => setMobileOpen(false)} />
          </SheetContent>
        </Sheet>
        <h1 className="text-lg font-semibold">{getPageTitle(pathname)}</h1>
      </div>
      <div className="flex items-center gap-2">
        {mounted && (
          <Button
            variant="ghost"
            size="icon"
            onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
          >
            {theme === "dark" ? (
              <Sun className="h-4 w-4" />
            ) : (
              <Moon className="h-4 w-4" />
            )}
          </Button>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button variant="ghost" className="relative h-8 w-8 rounded-full">
              <Avatar className="h-8 w-8">
                <AvatarFallback className="text-xs">{initials}</AvatarFallback>
              </Avatar>
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-56">
            <DropdownMenuLabel>
              <div className="flex flex-col space-y-1">
                <p className="text-sm font-medium">
                  {user?.full_name || "User"}
                </p>
                <p className="text-xs text-muted-foreground">{user?.email}</p>
              </div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => router.push("/settings")}>
              Settings
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={handleLogout}>Sign out</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    </header>
  );
}
