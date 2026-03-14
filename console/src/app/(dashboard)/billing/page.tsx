"use client";

import { useState, useMemo } from "react";
import { useBilling, useTransactions, useAllTransactions, useMe } from "@/lib/hooks";
import { topupCredits, updateAutoTopup, updateSpendLimit } from "@/lib/api";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { Badge } from "@/components/ui/badge";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  CreditCard,
  Zap,
  ArrowUpCircle,
  ArrowDownCircle,
  Gift,
  RefreshCw,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { toast } from "sonner";
import { formatDistanceToNow, format, parseISO, eachDayOfInterval, startOfDay } from "date-fns";
import dynamic from "next/dynamic";

const DailyCostChart = dynamic(() => import("./daily-cost-chart"), {
  ssr: false,
});

const TOPUP_PACKAGES = [
  { amount: 1_000, price: "$10" },
  { amount: 5_000, price: "$40", badge: "Save 20%" },
  { amount: 10_000, price: "$70", badge: "Save 30%" },
  { amount: 50_000, price: "$250", badge: "Save 50%" },
];

const TX_PAGE_SIZE = 20;

function transactionIcon(type: string) {
  switch (type) {
    case "initial_deposit":
      return <Gift className="h-4 w-4 text-green-500" />;
    case "manual_topup":
      return <ArrowUpCircle className="h-4 w-4 text-green-500" />;
    case "auto_topup":
      return <RefreshCw className="h-4 w-4 text-blue-500" />;
    case "usage_deduction":
      return <ArrowDownCircle className="h-4 w-4 text-red-500" />;
    default:
      return <CreditCard className="h-4 w-4 text-muted-foreground" />;
  }
}

function transactionLabel(type: string) {
  switch (type) {
    case "initial_deposit":
      return "Initial Deposit";
    case "manual_topup":
      return "Manual Top-up";
    case "auto_topup":
      return "Auto Top-up";
    case "usage_deduction":
      return "Usage";
    case "refund":
      return "Refund";
    case "adjustment":
      return "Adjustment";
    default:
      return type;
  }
}

export default function BillingPage() {
  const queryClient = useQueryClient();
  const { data: user, isLoading: userLoading } = useMe();
  const { data: billing, isLoading: billingLoading } = useBilling();
  const [txOffset, setTxOffset] = useState(0);
  const { data: txData, isLoading: txLoading } = useTransactions(TX_PAGE_SIZE, txOffset);
  const { data: allTxData } = useAllTransactions();

  // Auto top-up form state
  const [autoTopupAmount, setAutoTopupAmount] = useState("");
  const [autoTopupThreshold, setAutoTopupThreshold] = useState("");

  // Spend limit form state
  const [spendLimit, setSpendLimit] = useState("");

  const isLoading = userLoading || billingLoading;

  const topupMutation = useMutation({
    mutationFn: (amount: number) => topupCredits(amount),
    onSuccess: (data) => {
      toast.success(data.message);
      queryClient.invalidateQueries({ queryKey: ["billing"] });
      queryClient.invalidateQueries({ queryKey: ["transactions"] });
      queryClient.invalidateQueries({ queryKey: ["me"] });
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const autoTopupMutation = useMutation({
    mutationFn: (config: { enabled: boolean; amount?: number; threshold?: number }) =>
      updateAutoTopup(config),
    onSuccess: () => {
      toast.success("Auto top-up settings updated");
      queryClient.invalidateQueries({ queryKey: ["billing"] });
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const spendLimitMutation = useMutation({
    mutationFn: (limit: number | null) => updateSpendLimit(limit),
    onSuccess: () => {
      toast.success("Spend limit updated");
      queryClient.invalidateQueries({ queryKey: ["billing"] });
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const dailyCostData = useMemo(() => {
    if (!allTxData?.transactions.length) return [];

    const costByDay = new Map<string, number>();
    for (const tx of allTxData.transactions) {
      if (tx.amount >= 0) continue;
      const day = format(startOfDay(parseISO(tx.created_at)), "yyyy-MM-dd");
      costByDay.set(day, (costByDay.get(day) ?? 0) + Math.abs(tx.amount));
    }

    if (costByDay.size === 0) return [];

    const sortedDays = [...costByDay.keys()].sort();
    const start = parseISO(sortedDays[0]);
    const end = parseISO(sortedDays[sortedDays.length - 1]);
    const allDays = eachDayOfInterval({ start, end });

    return allDays.map((d) => {
      const key = format(d, "yyyy-MM-dd");
      return {
        date: format(d, "MMM d"),
        cost: costByDay.get(key) ?? 0,
      };
    });
  }, [allTxData]);

  if (isLoading) {
    return (
      <div className="space-y-6">
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  const creditsBalance = billing?.credits_balance ?? user?.account?.credits_balance ?? 0;

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Billing</h2>
        <p className="text-muted-foreground">
          Manage your credits, auto top-up, and spend limits
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
              {Number(creditsBalance).toLocaleString()}
            </span>
            <span className="text-muted-foreground">credits remaining</span>
          </div>
        </CardContent>
      </Card>

      {/* How credits work */}
      <Card>
        <CardHeader>
          <CardTitle>How credits work</CardTitle>
          <CardDescription>
            Credits are consumed per API call. Cost depends on the endpoint and features you enable.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-6">
          {/* /scrape */}
          <div className="space-y-2">
            <h4 className="text-sm font-semibold">/scrape</h4>
            <p className="text-sm text-muted-foreground">
              1 credit minimum per request. Base formats (<code className="text-xs bg-muted px-1 py-0.5 rounded">html</code>, <code className="text-xs bg-muted px-1 py-0.5 rounded">rawHtml</code>, <code className="text-xs bg-muted px-1 py-0.5 rounded">content</code>) are free.
            </p>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Component</TableHead>
                  <TableHead className="text-right">Credits</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow>
                  <TableCell className="text-sm">Each feature format: <code className="text-xs bg-muted px-1 py-0.5 rounded">markdown</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">links</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">metadata</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">screenshot</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">schema</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">blocks</code></TableCell>
                  <TableCell className="text-right font-mono">+1 each</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="text-sm">AI summary</TableCell>
                  <TableCell className="text-right font-mono">+5</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="text-sm">AI extraction</TableCell>
                  <TableCell className="text-right font-mono">+5</TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>

          {/* /map */}
          <div className="space-y-2">
            <h4 className="text-sm font-semibold">/map</h4>
            <p className="text-sm text-muted-foreground">
              Flat <span className="font-mono font-medium">2 credits</span> per call, regardless of the number of URLs discovered.
            </p>
          </div>

          {/* /crawl */}
          <div className="space-y-2">
            <h4 className="text-sm font-semibold">/crawl</h4>
            <p className="text-sm text-muted-foreground">
              Credits are deducted at job completion. Cost per page depends on crawler type and enabled features.
            </p>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Component</TableHead>
                  <TableHead className="text-right">Credits / page</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow>
                  <TableCell className="text-sm">HTTP mode (base)</TableCell>
                  <TableCell className="text-right font-mono">1</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="text-sm">Browser / JS mode (base)</TableCell>
                  <TableCell className="text-right font-mono">2</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="text-sm">Each feature: <code className="text-xs bg-muted px-1 py-0.5 rounded">metadata</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">markdown</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">block_split</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">schema</code> <code className="text-xs bg-muted px-1 py-0.5 rounded">custom_selectors</code></TableCell>
                  <TableCell className="text-right font-mono">+1 each</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="text-sm">AI extraction</TableCell>
                  <TableCell className="text-right font-mono">+5</TableCell>
                </TableRow>
                <TableRow>
                  <TableCell className="text-sm">AI summary</TableCell>
                  <TableCell className="text-right font-mono">+5</TableCell>
                </TableRow>
              </TableBody>
            </Table>
            <p className="text-xs text-muted-foreground">
              Formula: <code className="bg-muted px-1 py-0.5 rounded">total = pages_crawled × (base + feature_costs)</code>
            </p>
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
            Purchase credits to use across all features
          </CardDescription>
        </CardHeader>
        <CardContent>
          <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
            {TOPUP_PACKAGES.map((pack) => (
              <Button
                key={pack.amount}
                variant="outline"
                className="h-auto flex-col items-start gap-1 p-4"
                disabled={topupMutation.isPending}
                onClick={() => topupMutation.mutate(pack.amount)}
              >
                <div className="flex items-center gap-2">
                  <span className="text-lg font-bold">
                    {pack.amount.toLocaleString()} credits
                  </span>
                  {pack.badge && (
                    <Badge variant="secondary" className="text-xs">
                      {pack.badge}
                    </Badge>
                  )}
                </div>
                <span className="text-sm text-muted-foreground">{pack.price}</span>
              </Button>
            ))}
          </div>
        </CardContent>
      </Card>

      {/* Auto Top-up */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <RefreshCw className="h-5 w-5" />
            Auto Top-up
          </CardTitle>
          <CardDescription>
            Automatically add credits when your balance drops below a threshold
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center gap-3">
            <Switch
              checked={billing?.auto_topup_enabled ?? false}
              onCheckedChange={(enabled) => {
                const amount = autoTopupAmount ? parseInt(autoTopupAmount) : billing?.auto_topup_amount;
                const threshold = autoTopupThreshold ? parseInt(autoTopupThreshold) : billing?.auto_topup_threshold;
                autoTopupMutation.mutate({ enabled, amount, threshold });
              }}
            />
            <Label>
              {billing?.auto_topup_enabled ? "Enabled" : "Disabled"}
            </Label>
          </div>
          {billing?.auto_topup_enabled && (
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="topup-amount">Top-up amount (credits)</Label>
                <div className="flex gap-2">
                  <Input
                    id="topup-amount"
                    type="number"
                    min={1}
                    placeholder={String(billing.auto_topup_amount)}
                    value={autoTopupAmount}
                    onChange={(e) => setAutoTopupAmount(e.target.value)}
                  />
                  <Button
                    variant="secondary"
                    disabled={!autoTopupAmount || autoTopupMutation.isPending}
                    onClick={() => {
                      autoTopupMutation.mutate({
                        enabled: true,
                        amount: parseInt(autoTopupAmount),
                        threshold: autoTopupThreshold
                          ? parseInt(autoTopupThreshold)
                          : billing.auto_topup_threshold,
                      });
                      setAutoTopupAmount("");
                    }}
                  >
                    Save
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  Currently: {billing.auto_topup_amount.toLocaleString()} credits
                </p>
              </div>
              <div className="space-y-2">
                <Label htmlFor="topup-threshold">Trigger when balance below</Label>
                <div className="flex gap-2">
                  <Input
                    id="topup-threshold"
                    type="number"
                    min={0}
                    placeholder={String(billing.auto_topup_threshold)}
                    value={autoTopupThreshold}
                    onChange={(e) => setAutoTopupThreshold(e.target.value)}
                  />
                  <Button
                    variant="secondary"
                    disabled={!autoTopupThreshold || autoTopupMutation.isPending}
                    onClick={() => {
                      autoTopupMutation.mutate({
                        enabled: true,
                        amount: autoTopupAmount
                          ? parseInt(autoTopupAmount)
                          : billing.auto_topup_amount,
                        threshold: parseInt(autoTopupThreshold),
                      });
                      setAutoTopupThreshold("");
                    }}
                  >
                    Save
                  </Button>
                </div>
                <p className="text-xs text-muted-foreground">
                  Currently: {billing.auto_topup_threshold.toLocaleString()} credits
                </p>
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Monthly Spend Limit */}
      <Card>
        <CardHeader>
          <CardTitle>Monthly Spend Limit</CardTitle>
          <CardDescription>
            Set a maximum amount of credits that can be added per month via top-ups.
            Auto top-up will stop when this limit is reached.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex items-center gap-2">
            <Input
              type="number"
              min={0}
              placeholder={
                billing?.monthly_spend_limit
                  ? String(billing.monthly_spend_limit)
                  : "No limit"
              }
              value={spendLimit}
              onChange={(e) => setSpendLimit(e.target.value)}
              className="max-w-[200px]"
            />
            <span className="text-sm text-muted-foreground">credits / month</span>
            <Button
              variant="secondary"
              disabled={!spendLimit || spendLimitMutation.isPending}
              onClick={() => {
                const val = parseInt(spendLimit);
                spendLimitMutation.mutate(val > 0 ? val : null);
                setSpendLimit("");
              }}
            >
              Save
            </Button>
            {billing?.monthly_spend_limit && (
              <Button
                variant="ghost"
                size="sm"
                disabled={spendLimitMutation.isPending}
                onClick={() => {
                  spendLimitMutation.mutate(null);
                  setSpendLimit("");
                }}
              >
                Remove limit
              </Button>
            )}
          </div>
          {billing?.monthly_spend_limit && (
            <p className="text-sm text-muted-foreground">
              Current limit: {billing.monthly_spend_limit.toLocaleString()} credits / month
            </p>
          )}
        </CardContent>
      </Card>

      {/* Daily Cost Chart */}
      {dailyCostData.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>Daily Credit Usage</CardTitle>
            <CardDescription>
              Credits consumed per day
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="h-[250px]">
              <DailyCostChart data={dailyCostData} />
            </div>
          </CardContent>
        </Card>
      )}

      {/* Transaction History */}
      <Card>
        <CardHeader>
          <CardTitle>Transaction History</CardTitle>
          <CardDescription>
            All credit operations on your account
          </CardDescription>
        </CardHeader>
        <CardContent>
          {txLoading ? (
            <div className="space-y-2">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : !txData?.transactions.length ? (
            <p className="text-sm text-muted-foreground py-4 text-center">
              No transactions yet
            </p>
          ) : (
            <>
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Type</TableHead>
                    <TableHead>Description</TableHead>
                    <TableHead className="text-right">Amount</TableHead>
                    <TableHead className="text-right">Balance</TableHead>
                    <TableHead className="text-right">Date</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {txData.transactions.map((tx) => (
                    <TableRow key={tx.id}>
                      <TableCell>
                        <div className="flex items-center gap-2">
                          {transactionIcon(tx.type)}
                          <span className="text-sm font-medium">
                            {transactionLabel(tx.type)}
                          </span>
                        </div>
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        {tx.description || "—"}
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        <span
                          className={
                            tx.amount > 0
                              ? "text-green-600 dark:text-green-400"
                              : "text-red-600 dark:text-red-400"
                          }
                        >
                          {tx.amount > 0 ? "+" : ""}
                          {tx.amount.toLocaleString()}
                        </span>
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        {tx.balance_after.toLocaleString()}
                      </TableCell>
                      <TableCell className="text-right text-sm text-muted-foreground">
                        {formatDistanceToNow(new Date(tx.created_at), {
                          addSuffix: true,
                        })}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>

              {/* Pagination */}
              {txData.total > TX_PAGE_SIZE && (
                <div className="flex items-center justify-between pt-4">
                  <p className="text-sm text-muted-foreground">
                    Showing {txOffset + 1}–{Math.min(txOffset + TX_PAGE_SIZE, txData.total)} of{" "}
                    {txData.total}
                  </p>
                  <div className="flex gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={txOffset === 0}
                      onClick={() => setTxOffset(Math.max(0, txOffset - TX_PAGE_SIZE))}
                    >
                      <ChevronLeft className="h-4 w-4" />
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={txOffset + TX_PAGE_SIZE >= txData.total}
                      onClick={() => setTxOffset(txOffset + TX_PAGE_SIZE)}
                    >
                      <ChevronRight className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              )}
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
