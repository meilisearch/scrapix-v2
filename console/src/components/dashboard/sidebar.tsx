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
  FolderCog,
  BarChart3,
  Network,
  Cable,
  Moon,
  Sun,
  Zap,
  Coins,
  Search,
  Users,
  Plus,
  Check,
  ChevronsUpDown,
} from "lucide-react";
import { FeedbackDialog } from "@/components/dashboard/feedback-dialog";
import { logout } from "@/lib/auth";
import { useMe, useMyAccounts } from "@/lib/hooks";
import { useAccountStore } from "@/lib/account-store";
import { createAccount } from "@/lib/api";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
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
  { name: "Dashboard", href: "/dashboard", icon: LayoutDashboard },
];

const navGroups = [
  {
    label: "Playground",
    items: [
      { name: "Scrape", href: "/dashboard/scrape", icon: Globe },
      { name: "Map", href: "/dashboard/map", icon: Network },
      { name: "Search", href: "/dashboard/search", icon: Search },
      { name: "Crawl", href: "/dashboard/crawl", icon: Layers },
    ],
  },
  {
    label: "Management",
    items: [
      { name: "Jobs", href: "/dashboard/jobs", icon: ListTodo },
      { name: "Configs", href: "/dashboard/configs", icon: FolderCog },
    ],
  },
  {
    label: "Account",
    items: [
      { name: "Team", href: "/dashboard/team", icon: Users },
      { name: "API Keys", href: "/dashboard/api-keys", icon: Key },
      { name: "Billing", href: "/dashboard/billing", icon: CreditCard },
      { name: "Usage", href: "/dashboard/usage", icon: BarChart3 },
      { name: "MCP", href: "/dashboard/mcp", icon: Cable },
      { name: "Settings", href: "/dashboard/settings", icon: Settings },
    ],
  },
];

export function SidebarContent({ onNavigate }: { onNavigate?: () => void }) {
  const pathname = usePathname();
  const router = useRouter();
  const { resolvedTheme, theme, setTheme } = useTheme();
  const { data: user } = useMe();
  const { data: accounts } = useMyAccounts();
  const { selectedAccountId, setSelectedAccountId } = useAccountStore();
  const [mounted, setMounted] = useState(false);
  const [showCreateAccount, setShowCreateAccount] = useState(false);
  const [newAccountName, setNewAccountName] = useState("");
  const [creatingAccount, setCreatingAccount] = useState(false);
  const isDark = resolvedTheme === "dark" || resolvedTheme === "glitch";

  useEffect(() => {
    setMounted(true);
  }, []);

  const queryClient = useQueryClient();

  const handleLogout = async () => {
    await logout();
    router.push("/login");
    router.refresh();
  };

  const handleSwitchAccount = (accountId: string) => {
    setSelectedAccountId(accountId);
    queryClient.invalidateQueries();
  };

  const handleCreateAccount = async () => {
    if (!newAccountName.trim()) return;
    setCreatingAccount(true);
    try {
      const account = await createAccount(newAccountName.trim());
      queryClient.invalidateQueries({ queryKey: ["my-accounts"] });
      queryClient.invalidateQueries({ queryKey: ["me"] });
      setSelectedAccountId(account.id);
      queryClient.invalidateQueries();
      toast.success(`Account "${account.name}" created`);
      setNewAccountName("");
      setShowCreateAccount(false);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to create account");
    } finally {
      setCreatingAccount(false);
    }
  };

  const currentAccount = accounts?.find((a) => a.id === (selectedAccountId ?? user?.account?.id));

  const initials =
    user?.full_name
      ?.split(" ")
      .map((n: string) => n[0])
      .join("")
      .toUpperCase() || user?.email?.[0].toUpperCase() || "U";

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-14 items-center px-6 border-b">
        <Link href="/dashboard" className="glitch-logo-wrapper flex items-center" onClick={onNavigate}>
          <Image
            src={isDark ? "/logotype_dark.svg" : "/logotype_light.svg"}
            alt="Scrapix"
            width={140}
            height={36}
            className="h-8 w-auto glitch-logo"
          />
        </Link>
      </div>
      <nav className="min-h-0 flex-1 overflow-y-auto space-y-4 px-3 py-4">
        <div className="space-y-1">
          {topNav.map((item) => {
            const isActive = pathname === item.href;
            return (
              <Link
                key={item.name}
                href={item.href}
                onClick={onNavigate}
                className={cn(
                  "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors glitch-nav-item",
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
                pathname.startsWith(item.href + "/");
              return (
                <Link
                  key={item.name}
                  href={item.href}
                  onClick={onNavigate}
                  className={cn(
                    "flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors glitch-nav-item",
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
        <Link
          href="/dashboard/billing"
          onClick={onNavigate}
          className="flex items-center gap-2 rounded-lg border px-3 py-2 text-sm transition-colors hover:bg-muted"
        >
          <Coins className="h-4 w-4 text-primary" />
          <span className="font-medium">
            {user?.account?.credits_balance != null
              ? Number(user.account.credits_balance).toLocaleString()
              : "0"}
          </span>
          <span className="text-muted-foreground">credits</span>
        </Link>
        <FeedbackDialog onNavigate={onNavigate} />
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
            {theme === "glitch" ? "Glitch" : theme === "dark" ? "Dark" : "Light"} mode
          </Button>
        )}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              variant="ghost"
              className="w-full text-muted-foreground h-auto py-2 px-2 gap-2"
            >
              <Avatar className="h-6 w-6 shrink-0">
                <AvatarFallback className="text-[10px]">{initials}</AvatarFallback>
              </Avatar>
              <div className="flex flex-col items-start min-w-0 flex-1 overflow-hidden">
                <span className="truncate w-full text-left text-sm">{user?.full_name || user?.email || "Account"}</span>
                <span className="truncate w-full text-left text-[11px] text-muted-foreground/70">
                  {currentAccount?.name ?? user?.account?.name ?? ""}
                </span>
              </div>
              <ChevronsUpDown className="h-3 w-3 shrink-0 opacity-50" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent side="top" align="start" className="w-64">
            <DropdownMenuLabel>
              <div className="flex flex-col space-y-1">
                <p className="text-sm font-medium">
                  {user?.full_name || "User"}
                </p>
                <p className="text-xs text-muted-foreground">{user?.email}</p>
              </div>
            </DropdownMenuLabel>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => { router.push("/dashboard/settings"); onNavigate?.(); }}>
              Settings
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            {accounts && accounts.length > 0 && (
              <>
                <DropdownMenuLabel className="text-xs text-muted-foreground">
                  Switch account
                </DropdownMenuLabel>
                {accounts.map((account) => {
                  const isActive = account.id === (selectedAccountId ?? user?.account?.id);
                  return (
                    <DropdownMenuItem
                      key={account.id}
                      onClick={() => handleSwitchAccount(account.id)}
                      className={cn(isActive && "bg-muted")}
                    >
                      <span className="truncate flex-1">{account.name}</span>
                      {isActive && <Check className="ml-2 h-3.5 w-3.5 shrink-0" />}
                    </DropdownMenuItem>
                  );
                })}
                <DropdownMenuItem onClick={() => setShowCreateAccount(true)}>
                  <Plus className="mr-2 h-4 w-4" />
                  Create new account
                </DropdownMenuItem>
                <DropdownMenuSeparator />
              </>
            )}
            <DropdownMenuItem onClick={handleLogout}>Sign out</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <Dialog open={showCreateAccount} onOpenChange={setShowCreateAccount}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Create a new account</DialogTitle>
            <DialogDescription>
              Accounts are separate workspaces with their own billing, API keys, and team members.
            </DialogDescription>
          </DialogHeader>
          <div className="py-4">
            <Label htmlFor="account-name">Account name</Label>
            <Input
              id="account-name"
              placeholder="My Team"
              value={newAccountName}
              onChange={(e) => setNewAccountName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreateAccount()}
              className="mt-2"
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowCreateAccount(false)}>
              Cancel
            </Button>
            <Button onClick={handleCreateAccount} disabled={creatingAccount || !newAccountName.trim()}>
              {creatingAccount ? "Creating..." : "Create account"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
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
