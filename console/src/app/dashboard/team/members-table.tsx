"use client";

import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import type { MemberInfo } from "@/lib/api-types";
import { updateMemberRole, removeMember } from "@/lib/api";
import { useMe } from "@/lib/hooks";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
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
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { MoreHorizontal, Shield, ShieldCheck, Eye, UserMinus } from "lucide-react";

const ROLE_BADGES: Record<string, { variant: "default" | "secondary" | "outline"; label: string }> = {
  owner: { variant: "default", label: "Owner" },
  admin: { variant: "secondary", label: "Admin" },
  member: { variant: "outline", label: "Member" },
  viewer: { variant: "outline", label: "Viewer" },
};

export function MembersTable({ members }: { members: MemberInfo[] }) {
  const { data: user } = useMe();
  const queryClient = useQueryClient();
  const [confirmRemove, setConfirmRemove] = useState<MemberInfo | null>(null);
  const myRole = user?.account?.role ?? "viewer";
  const isOwner = myRole === "owner";

  const handleRoleChange = async (member: MemberInfo, newRole: string) => {
    try {
      await updateMemberRole(member.user_id, newRole);
      queryClient.invalidateQueries({ queryKey: ["members"] });
      toast.success(`Role updated to ${newRole}`);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to update role");
    }
  };

  const handleRemove = async () => {
    if (!confirmRemove) return;
    try {
      await removeMember(confirmRemove.user_id);
      queryClient.invalidateQueries({ queryKey: ["members"] });
      queryClient.invalidateQueries({ queryKey: ["me"] });
      toast.success("Member removed");
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to remove member");
    } finally {
      setConfirmRemove(null);
    }
  };

  return (
    <>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Member</TableHead>
            <TableHead>Role</TableHead>
            <TableHead>Joined</TableHead>
            {isOwner && <TableHead className="w-10" />}
          </TableRow>
        </TableHeader>
        <TableBody>
          {members.map((m) => {
            const initials = m.full_name
              ? m.full_name.split(" ").map((n) => n[0]).join("").toUpperCase()
              : m.email[0].toUpperCase();
            const badge = ROLE_BADGES[m.role] ?? ROLE_BADGES.member;
            const isMe = m.user_id === user?.id;

            return (
              <TableRow key={m.user_id}>
                <TableCell>
                  <div className="flex items-center gap-3">
                    <Avatar className="h-8 w-8">
                      <AvatarFallback className="text-xs">{initials}</AvatarFallback>
                    </Avatar>
                    <div>
                      <p className="text-sm font-medium">
                        {m.full_name ?? m.email}
                        {isMe && (
                          <span className="ml-2 text-xs text-muted-foreground">(you)</span>
                        )}
                      </p>
                      {m.full_name && (
                        <p className="text-xs text-muted-foreground">{m.email}</p>
                      )}
                    </div>
                  </div>
                </TableCell>
                <TableCell>
                  <Badge variant={badge.variant}>{badge.label}</Badge>
                </TableCell>
                <TableCell className="text-sm text-muted-foreground">
                  {new Date(m.joined_at).toLocaleDateString()}
                </TableCell>
                {isOwner && (
                  <TableCell>
                    {!isMe && m.role !== "owner" && (
                      <DropdownMenu>
                        <DropdownMenuTrigger asChild>
                          <Button variant="ghost" size="icon" className="h-8 w-8">
                            <MoreHorizontal className="h-4 w-4" />
                          </Button>
                        </DropdownMenuTrigger>
                        <DropdownMenuContent align="end">
                          <DropdownMenuItem onClick={() => handleRoleChange(m, "admin")}>
                            <ShieldCheck className="mr-2 h-4 w-4" />
                            Set Admin
                          </DropdownMenuItem>
                          <DropdownMenuItem onClick={() => handleRoleChange(m, "member")}>
                            <Shield className="mr-2 h-4 w-4" />
                            Set Member
                          </DropdownMenuItem>
                          <DropdownMenuItem onClick={() => handleRoleChange(m, "viewer")}>
                            <Eye className="mr-2 h-4 w-4" />
                            Set Viewer
                          </DropdownMenuItem>
                          <DropdownMenuSeparator />
                          <DropdownMenuItem
                            className="text-destructive"
                            onClick={() => setConfirmRemove(m)}
                          >
                            <UserMinus className="mr-2 h-4 w-4" />
                            Remove
                          </DropdownMenuItem>
                        </DropdownMenuContent>
                      </DropdownMenu>
                    )}
                  </TableCell>
                )}
              </TableRow>
            );
          })}
        </TableBody>
      </Table>

      <AlertDialog open={!!confirmRemove} onOpenChange={() => setConfirmRemove(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove member</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to remove{" "}
              <strong>{confirmRemove?.full_name ?? confirmRemove?.email}</strong> from this
              account? They will lose access immediately.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleRemove} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
