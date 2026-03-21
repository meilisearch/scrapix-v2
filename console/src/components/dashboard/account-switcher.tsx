"use client";

import { useState } from "react";
import { useMyAccounts, useMe } from "@/lib/hooks";
import { useAccountStore } from "@/lib/account-store";
import { useQueryClient } from "@tanstack/react-query";
import { createAccount } from "@/lib/api";
import { cn } from "@/lib/utils";
import { Check, ChevronsUpDown, Plus } from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { toast } from "sonner";

export function AccountSwitcher() {
  const { data: accounts } = useMyAccounts();
  const { data: user } = useMe();
  const queryClient = useQueryClient();
  const { selectedAccountId, setSelectedAccountId } = useAccountStore();
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState("");
  const [creating, setCreating] = useState(false);

  const currentAccount = accounts?.find((a) => a.id === (selectedAccountId ?? user?.account?.id));

  const handleSwitch = (accountId: string) => {
    setSelectedAccountId(accountId);
    queryClient.invalidateQueries();
  };

  const handleCreate = async () => {
    if (!newName.trim()) return;
    setCreating(true);
    try {
      const account = await createAccount(newName.trim());
      queryClient.invalidateQueries({ queryKey: ["my-accounts"] });
      queryClient.invalidateQueries({ queryKey: ["me"] });
      setSelectedAccountId(account.id);
      queryClient.invalidateQueries();
      toast.success(`Account "${account.name}" created`);
      setNewName("");
      setShowCreate(false);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to create account");
    } finally {
      setCreating(false);
    }
  };

  // Always show — even with 1 account, user can create new ones
  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button variant="outline" className="w-full justify-between text-sm h-9 px-3">
            <span className="truncate">{currentAccount?.name ?? user?.account?.name ?? "Account"}</span>
            <ChevronsUpDown className="ml-2 h-3 w-3 shrink-0 opacity-50" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="w-64">
          <DropdownMenuLabel>Switch account</DropdownMenuLabel>
          <DropdownMenuSeparator />
          {accounts?.map((account) => {
            const isActive = account.id === (selectedAccountId ?? user?.account?.id);
            return (
              <DropdownMenuItem
                key={account.id}
                onClick={() => handleSwitch(account.id)}
                className={cn("flex items-center justify-between", isActive && "bg-muted")}
              >
                <div className="flex flex-col gap-0.5 min-w-0">
                  <span className="truncate font-medium">{account.name}</span>
                  <span className="text-xs text-muted-foreground">{account.role}</span>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  <Badge variant="outline" className="text-[10px]">
                    {account.tier}
                  </Badge>
                  {isActive && <Check className="h-3.5 w-3.5" />}
                </div>
              </DropdownMenuItem>
            );
          })}
          <DropdownMenuSeparator />
          <DropdownMenuItem onClick={() => setShowCreate(true)}>
            <Plus className="mr-2 h-4 w-4" />
            Create new account
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>

      <Dialog open={showCreate} onOpenChange={setShowCreate}>
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
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleCreate()}
              className="mt-2"
            />
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowCreate(false)}>
              Cancel
            </Button>
            <Button onClick={handleCreate} disabled={creating || !newName.trim()}>
              {creating ? "Creating..." : "Create account"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
