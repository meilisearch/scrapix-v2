"use client";

import { useMe, useMembers, useInvites } from "@/lib/hooks";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { MembersTable } from "./members-table";
import { InviteDialog } from "./invite-dialog";
import { PendingInvites } from "./pending-invites";

export default function TeamPage() {
  const { data: user } = useMe();
  const { data: members, isLoading: membersLoading } = useMembers();
  const { data: invites, isLoading: invitesLoading } = useInvites();
  const myRole = user?.account?.role ?? "viewer";
  const canInvite = myRole === "owner" || myRole === "admin";

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Team</h1>
          <p className="text-muted-foreground">
            Manage who has access to this account.
          </p>
        </div>
        {canInvite && <InviteDialog />}
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Members</CardTitle>
          <CardDescription>
            People who have access to this account and its resources.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {membersLoading ? (
            <div className="space-y-3">
              {[...Array(3)].map((_, i) => (
                <Skeleton key={i} className="h-12 w-full" />
              ))}
            </div>
          ) : members && members.length > 0 ? (
            <MembersTable members={members} />
          ) : (
            <p className="text-sm text-muted-foreground py-4 text-center">
              No members found.
            </p>
          )}
        </CardContent>
      </Card>

      {canInvite && (
        <Card>
          <CardHeader>
            <CardTitle>Invitations</CardTitle>
            <CardDescription>
              Pending invites that haven&apos;t been accepted yet.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {invitesLoading ? (
              <div className="space-y-3">
                <Skeleton className="h-12 w-full" />
              </div>
            ) : invites && invites.length > 0 ? (
              <PendingInvites invites={invites} />
            ) : (
              <p className="text-sm text-muted-foreground py-4 text-center">
                No pending invites.
              </p>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
