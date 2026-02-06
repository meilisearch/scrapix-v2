"use client";

import { useEffect, useState } from "react";
import { createClient } from "@/lib/supabase/client";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Plus, Copy, Eye, EyeOff, Trash2, Loader2 } from "lucide-react";
import { toast } from "sonner";
import { formatDistanceToNow } from "date-fns";

interface ApiKey {
  id: string;
  name: string;
  prefix: string;
  active: boolean;
  last_used_at: string | null;
  created_at: string;
}

export default function ApiKeysPage() {
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [loading, setLoading] = useState(true);
  const [accountId, setAccountId] = useState<string | null>(null);
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [createdKey, setCreatedKey] = useState<string | null>(null);
  const [showKey, setShowKey] = useState(false);
  const [creating, setCreating] = useState(false);
  const supabase = createClient();

  useEffect(() => {
    fetchKeys();
  }, []);

  const fetchKeys = async () => {
    const {
      data: { user },
    } = await supabase.auth.getUser();
    if (!user) return;

    // Get the user's account
    const { data: membership } = await supabase
      .from("account_members")
      .select("account_id")
      .eq("user_id", user.id)
      .single();

    if (membership) {
      setAccountId(membership.account_id);

      // Get API keys for this account
      const { data: keysData } = await supabase
        .from("api_keys")
        .select("id, name, prefix, active, last_used_at, created_at")
        .eq("account_id", membership.account_id)
        .order("created_at", { ascending: false });

      if (keysData) {
        setKeys(keysData);
      }
    }
    setLoading(false);
  };

  const generateApiKey = () => {
    const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let key = "sk_live_";
    for (let i = 0; i < 32; i++) {
      key += chars.charAt(Math.floor(Math.random() * chars.length));
    }
    return key;
  };

  const hashKey = async (key: string) => {
    const encoder = new TextEncoder();
    const data = encoder.encode(key);
    const hashBuffer = await crypto.subtle.digest("SHA-256", data);
    const hashArray = Array.from(new Uint8Array(hashBuffer));
    return hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");
  };

  const createKey = async () => {
    if (!accountId || !newKeyName.trim()) return;

    setCreating(true);
    const apiKey = generateApiKey();
    const keyHash = await hashKey(apiKey);
    const prefix = apiKey.slice(0, 12) + "...";

    const { error } = await supabase.from("api_keys").insert({
      account_id: accountId,
      name: newKeyName.trim(),
      prefix,
      key_hash: keyHash,
    });

    if (error) {
      toast.error("Failed to create API key");
      setCreating(false);
      return;
    }

    setCreatedKey(apiKey);
    setNewKeyName("");
    setCreating(false);
    fetchKeys();
  };

  const revokeKey = async (keyId: string) => {
    const { error } = await supabase
      .from("api_keys")
      .update({ active: false })
      .eq("id", keyId);

    if (error) {
      toast.error("Failed to revoke API key");
      return;
    }

    toast.success("API key revoked");
    fetchKeys();
  };

  const copyToClipboard = (text: string) => {
    navigator.clipboard.writeText(text);
    toast.success("Copied to clipboard");
  };

  const closeCreateDialog = () => {
    setCreateDialogOpen(false);
    setCreatedKey(null);
    setNewKeyName("");
    setShowKey(false);
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-2xl font-bold tracking-tight">API Keys</h2>
          <p className="text-muted-foreground">
            Manage your API keys for authenticating with the Scrapix API
          </p>
        </div>
        <Dialog open={createDialogOpen} onOpenChange={setCreateDialogOpen}>
          <DialogTrigger asChild>
            <Button>
              <Plus className="mr-2 h-4 w-4" />
              Create API Key
            </Button>
          </DialogTrigger>
          <DialogContent>
            {createdKey ? (
              <>
                <DialogHeader>
                  <DialogTitle>API Key Created</DialogTitle>
                  <DialogDescription>
                    Copy your API key now. You won&apos;t be able to see it
                    again!
                  </DialogDescription>
                </DialogHeader>
                <div className="space-y-4 py-4">
                  <div className="p-4 bg-muted rounded-lg">
                    <div className="flex items-center gap-2">
                      <code className="flex-1 text-sm break-all">
                        {showKey ? createdKey : "•".repeat(40)}
                      </code>
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => setShowKey(!showKey)}
                      >
                        {showKey ? (
                          <EyeOff className="h-4 w-4" />
                        ) : (
                          <Eye className="h-4 w-4" />
                        )}
                      </Button>
                      <Button
                        variant="ghost"
                        size="icon"
                        onClick={() => copyToClipboard(createdKey)}
                      >
                        <Copy className="h-4 w-4" />
                      </Button>
                    </div>
                  </div>
                  <p className="text-sm text-muted-foreground">
                    Store this key securely. For security, we cannot show it
                    again.
                  </p>
                </div>
                <DialogFooter>
                  <Button onClick={closeCreateDialog}>Done</Button>
                </DialogFooter>
              </>
            ) : (
              <>
                <DialogHeader>
                  <DialogTitle>Create API Key</DialogTitle>
                  <DialogDescription>
                    Give your API key a name to help you identify it later
                  </DialogDescription>
                </DialogHeader>
                <div className="space-y-4 py-4">
                  <div className="space-y-2">
                    <Label htmlFor="name">Name</Label>
                    <Input
                      id="name"
                      placeholder="e.g., Production, Development"
                      value={newKeyName}
                      onChange={(e) => setNewKeyName(e.target.value)}
                    />
                  </div>
                </div>
                <DialogFooter>
                  <Button variant="outline" onClick={closeCreateDialog}>
                    Cancel
                  </Button>
                  <Button
                    onClick={createKey}
                    disabled={!newKeyName.trim() || creating}
                  >
                    {creating && (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    )}
                    Create Key
                  </Button>
                </DialogFooter>
              </>
            )}
          </DialogContent>
        </Dialog>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Your API Keys</CardTitle>
          <CardDescription>
            Use these keys to authenticate API requests
          </CardDescription>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="flex justify-center py-8">
              <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
            </div>
          ) : keys.length === 0 ? (
            <div className="text-center py-8">
              <p className="text-muted-foreground">
                No API keys yet. Create one to get started.
              </p>
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Key</TableHead>
                  <TableHead>Status</TableHead>
                  <TableHead>Last Used</TableHead>
                  <TableHead>Created</TableHead>
                  <TableHead className="text-right">Actions</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {keys.map((key) => (
                  <TableRow key={key.id}>
                    <TableCell className="font-medium">{key.name}</TableCell>
                    <TableCell>
                      <code className="text-sm bg-muted px-2 py-1 rounded">
                        {key.prefix}
                      </code>
                    </TableCell>
                    <TableCell>
                      <Badge variant={key.active ? "default" : "secondary"}>
                        {key.active ? "Active" : "Revoked"}
                      </Badge>
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {key.last_used_at
                        ? formatDistanceToNow(new Date(key.last_used_at), {
                            addSuffix: true,
                          })
                        : "Never"}
                    </TableCell>
                    <TableCell className="text-muted-foreground">
                      {formatDistanceToNow(new Date(key.created_at), {
                        addSuffix: true,
                      })}
                    </TableCell>
                    <TableCell className="text-right">
                      {key.active && (
                        <Button
                          variant="ghost"
                          size="icon"
                          className="text-destructive hover:text-destructive"
                          onClick={() => revokeKey(key.id)}
                        >
                          <Trash2 className="h-4 w-4" />
                        </Button>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Using Your API Key</CardTitle>
          <CardDescription>
            Include your API key in the X-API-Key header of all requests
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="bg-muted rounded-lg p-4">
            <code className="text-sm">
              curl -X POST https://api.scrapix.io/scrape \<br />
              &nbsp;&nbsp;-H &quot;X-API-Key: sk_live_...&quot; \<br />
              &nbsp;&nbsp;-H &quot;Content-Type: application/json&quot; \<br />
              &nbsp;&nbsp;-d &apos;{`{"url": "https://example.com"}`}&apos;
            </code>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
