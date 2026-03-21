"use client";

import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { InviteInfo } from "@/lib/api-types";
import { revokeInvite } from "@/lib/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { X } from "lucide-react";

export function PendingInvites({ invites }: { invites: InviteInfo[] }) {
  const queryClient = useQueryClient();

  if (invites.length === 0) return null;

  const handleRevoke = async (invite: InviteInfo) => {
    try {
      await revokeInvite(invite.id);
      queryClient.invalidateQueries({ queryKey: ["invites"] });
      toast.success(`Invite to ${invite.email} revoked`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to revoke invite");
    }
  };

  return (
    <div className="space-y-3">
      <h3 className="text-sm font-medium text-muted-foreground">Pending invites</h3>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Email</TableHead>
            <TableHead>Role</TableHead>
            <TableHead>Expires</TableHead>
            <TableHead className="w-10" />
          </TableRow>
        </TableHeader>
        <TableBody>
          {invites.map((inv) => (
            <TableRow key={inv.id}>
              <TableCell className="text-sm">{inv.email}</TableCell>
              <TableCell>
                <Badge variant="outline" className="capitalize">
                  {inv.role}
                </Badge>
              </TableCell>
              <TableCell className="text-sm text-muted-foreground">
                {new Date(inv.expires_at).toLocaleDateString()}
              </TableCell>
              <TableCell>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-8 w-8 text-muted-foreground hover:text-destructive"
                  onClick={() => handleRevoke(inv)}
                >
                  <X className="h-4 w-4" />
                </Button>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
