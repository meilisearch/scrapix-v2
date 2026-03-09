"use client";

import { useMe } from "@/lib/hooks";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { CreditCard, Zap, ArrowRight } from "lucide-react";

export default function BillingPage() {
  const { data: user, isLoading } = useMe();

  if (isLoading) {
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
        <h2 className="text-2xl font-bold tracking-tight">Billing</h2>
        <p className="text-muted-foreground">
          Manage your credits and payment methods
        </p>
      </div>

      {/* Credits Balance */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Zap className="h-5 w-5" />
            Credits Balance
          </CardTitle>
          <CardDescription>
            Credits are consumed per crawl request. All features are available with credits.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="flex items-baseline gap-2">
            <span className="text-4xl font-bold">
              {user?.account?.credits_balance != null
                ? Number(user.account.credits_balance).toLocaleString()
                : "0"}
            </span>
            <span className="text-muted-foreground">credits remaining</span>
          </div>
        </CardContent>
      </Card>

      {/* Pricing */}
      <Card>
        <CardHeader>
          <CardTitle>How credits work</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-4 sm:grid-cols-3">
            <div className="rounded-lg border p-4 space-y-1">
              <p className="text-sm font-medium">Page crawl</p>
              <p className="text-2xl font-bold">1 credit</p>
              <p className="text-xs text-muted-foreground">Per page fetched</p>
            </div>
            <div className="rounded-lg border p-4 space-y-1">
              <p className="text-sm font-medium">JS rendering</p>
              <p className="text-2xl font-bold">5 credits</p>
              <p className="text-xs text-muted-foreground">Per page with browser</p>
            </div>
            <div className="rounded-lg border p-4 space-y-1">
              <p className="text-sm font-medium">AI extraction</p>
              <p className="text-2xl font-bold">10 credits</p>
              <p className="text-xs text-muted-foreground">Per AI-processed page</p>
            </div>
          </div>
          <p className="text-sm text-muted-foreground">
            No feature restrictions. Everything is available — you only pay for what you use.
          </p>
        </CardContent>
      </Card>

      {/* Add Credits */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <CreditCard className="h-5 w-5" />
            Add Credits
          </CardTitle>
          <CardDescription>
            Prepay credits to use across all features
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-3">
            {[
              { amount: 1_000, price: "$10" },
              { amount: 10_000, price: "$80", badge: "Save 20%" },
              { amount: 100_000, price: "$500", badge: "Save 50%" },
            ].map((pack) => (
              <Button
                key={pack.amount}
                variant="outline"
                className="h-auto flex-col items-start gap-1 p-4"
                disabled
              >
                <div className="flex items-center gap-2">
                  <span className="text-lg font-bold">
                    {pack.amount.toLocaleString()} credits
                  </span>
                </div>
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium">{pack.price}</span>
                  {pack.badge && (
                    <span className="text-xs text-primary">{pack.badge}</span>
                  )}
                </div>
                <span className="text-xs text-muted-foreground flex items-center gap-1">
                  Coming soon <ArrowRight className="h-3 w-3" />
                </span>
              </Button>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
