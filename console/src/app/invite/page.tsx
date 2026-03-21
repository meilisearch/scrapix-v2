"use client";

import { Suspense, useEffect, useState } from "react";
import { useSearchParams, useRouter } from "next/navigation";
import { acceptInvite } from "@/lib/api";
import { useMe } from "@/lib/hooks";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Loader2, CheckCircle, XCircle } from "lucide-react";

function AcceptInviteContent() {
  const searchParams = useSearchParams();
  const router = useRouter();
  const token = searchParams.get("token");
  const { data: user, isLoading: userLoading } = useMe();
  const [status, setStatus] = useState<"loading" | "success" | "error" | "no-token">("loading");
  const [message, setMessage] = useState("");

  useEffect(() => {
    if (!token) {
      setStatus("no-token");
      return;
    }

    if (userLoading) return;

    if (!user) {
      router.replace(`/login?redirect=${encodeURIComponent(`/invite?token=${token}`)}`);
      return;
    }

    acceptInvite(token)
      .then((res) => {
        setStatus("success");
        setMessage(res.message);
      })
      .catch((e) => {
        setStatus("error");
        setMessage(e instanceof Error ? e.message : "Failed to accept invite");
      });
  }, [token, user, userLoading, router]);

  return (
    <Card className="w-full max-w-md">
      <CardHeader className="text-center">
        <CardTitle>Team Invite</CardTitle>
        <CardDescription>
          {status === "loading" && "Processing your invitation..."}
          {status === "no-token" && "No invite token provided."}
          {status === "success" && "You're in!"}
          {status === "error" && "Something went wrong"}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col items-center gap-4">
        {status === "loading" && <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />}
        {status === "success" && (
          <>
            <CheckCircle className="h-10 w-10 text-green-500" />
            <p className="text-sm text-muted-foreground text-center">{message}</p>
            <Button onClick={() => router.push("/dashboard")}>Go to Dashboard</Button>
          </>
        )}
        {status === "error" && (
          <>
            <XCircle className="h-10 w-10 text-destructive" />
            <p className="text-sm text-muted-foreground text-center">{message}</p>
            <Button variant="outline" onClick={() => router.push("/dashboard")}>
              Go to Dashboard
            </Button>
          </>
        )}
        {status === "no-token" && (
          <Button variant="outline" onClick={() => router.push("/dashboard")}>
            Go to Dashboard
          </Button>
        )}
      </CardContent>
    </Card>
  );
}

export default function AcceptInvitePage() {
  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-4">
      <Suspense fallback={
        <Card className="w-full max-w-md">
          <CardContent className="flex items-center justify-center py-12">
            <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
          </CardContent>
        </Card>
      }>
        <AcceptInviteContent />
      </Suspense>
    </div>
  );
}
