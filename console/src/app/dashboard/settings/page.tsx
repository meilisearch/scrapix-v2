"use client";

import { useState, useEffect } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useMe } from "@/lib/hooks";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Badge } from "@/components/ui/badge";
import { Loader2, Database, CheckCircle } from "lucide-react";
import { toast } from "sonner";
import { Skeleton } from "@/components/ui/skeleton";
import { fetchEngines, createEngine, updateEngine } from "@/lib/api";
import type { MeilisearchEngine } from "@/lib/api-types";

const BASE = "/api/scrapix";

export default function SettingsPage() {
  const queryClient = useQueryClient();
  const { data: user, isLoading: loading } = useMe();
  const [savingProfile, setSavingProfile] = useState(false);
  const [savingAccount, setSavingAccount] = useState(false);
  const [fullName, setFullName] = useState("");
  const [accountName, setAccountName] = useState("");
  const [initialized, setInitialized] = useState(false);

  if (user && !initialized) {
    setFullName(user.full_name || "");
    setAccountName(user.account?.name || "");
    setInitialized(true);
  }

  const saveProfile = async () => {
    setSavingProfile(true);
    try {
      const res = await fetch(`${BASE}/auth/me`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ full_name: fullName }),
        credentials: "include",
      });
      if (!res.ok) throw new Error();
      toast.success("Profile updated");
      queryClient.invalidateQueries({ queryKey: ["me"] });
    } catch {
      toast.error("Failed to update profile");
    }
    setSavingProfile(false);
  };

  const saveAccount = async () => {
    setSavingAccount(true);
    try {
      const res = await fetch(`${BASE}/account`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: accountName }),
        credentials: "include",
      });
      if (!res.ok) throw new Error();
      toast.success("Account updated");
      queryClient.invalidateQueries({ queryKey: ["me"] });
    } catch {
      toast.error("Failed to update account");
    }
    setSavingAccount(false);
  };

  const isOwner = user?.account?.role === "owner";

  if (loading) {
    return (
      <div className="space-y-6">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Settings</h2>
        <p className="text-muted-foreground">
          Manage your account settings and preferences
        </p>
      </div>

      {/* Profile Settings */}
      <Card>
        <CardHeader>
          <CardTitle>Profile</CardTitle>
          <CardDescription>
            Update your personal information
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="email">Email</Label>
            <Input
              id="email"
              type="email"
              value={user?.email || ""}
              disabled
              className="bg-muted"
            />
            <p className="text-xs text-muted-foreground">
              Email cannot be changed
            </p>
          </div>
          <div className="space-y-2">
            <Label htmlFor="fullName">Full Name</Label>
            <Input
              id="fullName"
              value={fullName}
              onChange={(e) => setFullName(e.target.value)}
            />
          </div>
        </CardContent>
        <CardFooter>
          <Button
            onClick={saveProfile}
            disabled={savingProfile || fullName === user?.full_name}
          >
            {savingProfile && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            Save Changes
          </Button>
        </CardFooter>
      </Card>

      {/* Account Settings */}
      <Card>
        <CardHeader>
          <CardTitle>Account</CardTitle>
          <CardDescription>
            Manage your organization settings
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="space-y-2">
            <Label htmlFor="accountId">Account ID</Label>
            <Input
              id="accountId"
              value={user?.account?.id || ""}
              disabled
              className="bg-muted font-mono"
            />
          </div>
          <div className="space-y-2">
            <Label htmlFor="accountName">Account Name</Label>
            <Input
              id="accountName"
              value={accountName}
              onChange={(e) => setAccountName(e.target.value)}
              disabled={!isOwner}
            />
            {!isOwner && (
              <p className="text-xs text-muted-foreground">Only the account owner can change this.</p>
            )}
          </div>
        </CardContent>
        {isOwner && (
          <CardFooter>
            <Button
              onClick={saveAccount}
              disabled={savingAccount || accountName === user?.account?.name}
            >
              {savingAccount && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
              Save Changes
            </Button>
          </CardFooter>
        )}
      </Card>

      {/* Meilisearch Engine */}
      <MeilisearchEngineCard />

      {isOwner && <Separator />}

      {/* Danger Zone — owner only */}
      {isOwner && <Card className="border-destructive">
        <CardHeader>
          <CardTitle className="text-destructive">Danger Zone</CardTitle>
          <CardDescription>
            Irreversible and destructive actions
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <p className="font-medium">Delete Account</p>
              <p className="text-sm text-muted-foreground">
                Permanently delete your account and all associated data
              </p>
            </div>
            <Button variant="destructive" disabled title="Coming soon">
              Delete Account
            </Button>
          </div>
        </CardContent>
      </Card>}
    </div>
  );
}

function MeilisearchEngineCard() {
  const queryClient = useQueryClient();
  const { data: engines = [], isLoading } = useQuery({
    queryKey: ["engines"],
    queryFn: fetchEngines,
    staleTime: 60_000,
  });

  const defaultEngine = engines.find((e: MeilisearchEngine) => e.is_default) || engines[0];

  const [msUrl, setMsUrl] = useState("");
  const [msApiKey, setMsApiKey] = useState("");
  const [saving, setSaving] = useState(false);
  const [engineInitialized, setEngineInitialized] = useState(false);

  useEffect(() => {
    if (engineInitialized || !defaultEngine) return;
    setMsUrl(defaultEngine.url);
    setMsApiKey(defaultEngine.api_key);
    setEngineInitialized(true);
  }, [defaultEngine, engineInitialized]);

  const hasChanges =
    defaultEngine &&
    (msUrl !== defaultEngine.url || msApiKey !== defaultEngine.api_key);

  const isNew = engines.length === 0 && !isLoading;

  const handleSave = async () => {
    if (!msUrl.trim()) {
      toast.error("Meilisearch URL is required");
      return;
    }

    setSaving(true);
    try {
      if (defaultEngine) {
        await updateEngine(defaultEngine.id, {
          url: msUrl.trim(),
          api_key: msApiKey,
        });
      } else {
        await createEngine({
          name: "Default",
          url: msUrl.trim(),
          api_key: msApiKey || undefined,
          is_default: true,
        });
      }
      toast.success("Meilisearch engine saved");
      queryClient.invalidateQueries({ queryKey: ["engines"] });
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to save engine";
      toast.error(msg);
    }
    setSaving(false);
  };

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <Database className="h-5 w-5 text-muted-foreground" />
          <CardTitle>Meilisearch</CardTitle>
          {defaultEngine && (
            <Badge variant="outline" className="ml-auto gap-1 text-xs">
              <CheckCircle className="h-3 w-3 text-green-500" />
              Connected
            </Badge>
          )}
        </div>
        <CardDescription>
          Configure the Meilisearch instance used for indexing crawled content and serving search results.
          All crawl jobs and search queries will use this engine.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {isLoading ? (
          <div className="space-y-3">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        ) : (
          <>
            <div className="space-y-2">
              <Label htmlFor="ms-url">URL</Label>
              <Input
                id="ms-url"
                placeholder="https://your-instance.meilisearch.com"
                value={msUrl}
                onChange={(e) => setMsUrl(e.target.value)}
                className="font-mono text-sm"
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="ms-api-key">API Key</Label>
              <Input
                id="ms-api-key"
                type="password"
                placeholder="Enter your Meilisearch API key"
                value={msApiKey}
                onChange={(e) => setMsApiKey(e.target.value)}
                className="font-mono text-sm"
              />
              <p className="text-xs text-muted-foreground">
                Use a key with read and write permissions on all indexes.
              </p>
            </div>
          </>
        )}
      </CardContent>
      <CardFooter>
        <Button
          onClick={handleSave}
          disabled={saving || (!isNew && !hasChanges)}
        >
          {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          {isNew ? "Connect Engine" : "Save Changes"}
        </Button>
      </CardFooter>
    </Card>
  );
}
