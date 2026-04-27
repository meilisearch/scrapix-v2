"use client";

import { useState, useMemo } from "react";
import { useBilling, useTransactions, useAllTransactions, useMe } from "@/lib/hooks";
import { fetchBillingPortal, updateSpendLimit } from "@/lib/api";
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
import { Skeleton } from "@/components/ui/skeleton";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
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
  Loader2,
  Info,
  ExternalLink,
  AlertTriangle,
  Receipt,
} from "lucide-react";
import { toast } from "sonner";
import { formatDistanceToNow, format, parseISO, eachDayOfInterval, startOfDay } from "date-fns";
import dynamic from "next/dynamic";

const DailyCostChart = dynamic(() => import("./daily-cost-chart"), {
  ssr: false,
});

const TX_PAGE_SIZE = 5;

// Format a smallest-currency-unit amount (cents for USD) as a human-readable
// string. Falls back gracefully when `currency` is missing.
function formatMoney(amount: number, currency: string | null | undefined): string {
  const code = (currency ?? "USD").toUpperCase();
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: code,
    }).format(amount / 100);
  } catch {
    // Non-ISO currency code — render the raw amount and the code.
    return `${(amount / 100).toFixed(2)} ${code}`;
  }
}

function transactionIcon(type: string) {
  switch (type) {
    case "initial_deposit":
      return <Gift className="h-4 w-4 text-green-500" />;
    case "manual_topup":
    case "wallet_credit":
      return <ArrowUpCircle className="h-4 w-4 text-green-500" />;
    case "auto_topup":
      return <RefreshCw className="h-4 w-4 text-blue-500" />;
    case "usage_deduction":
      return <ArrowDownCircle className="h-4 w-4 text-red-500" />;
    case "invoice_settled":
      return <Receipt className="h-4 w-4 text-green-500" />;
    case "invoice_errored":
      return <AlertTriangle className="h-4 w-4 text-red-500" />;
    default:
      return <CreditCard className="h-4 w-4 text-muted-foreground" />;
  }
}

function transactionLabel(type: string) {
  switch (type) {
    case "initial_deposit":
      return "Initial Deposit";
    case "manual_topup":
      return "Credit Purchase";
    case "auto_topup":
      return "Auto Top-up";
    case "usage_deduction":
      return "Usage";
    case "wallet_credit":
      return "Wallet Credit";
    case "invoice_settled":
      return "Invoice Paid";
    case "invoice_errored":
      return "Invoice Failed";
    case "refund":
      return "Refund";
    case "adjustment":
      return "Adjustment";
    default:
      return type;
  }
}

// ============================================================================
// Main Billing Page
// ============================================================================

export default function BillingPage() {
  const queryClient = useQueryClient();
  const { data: user, isLoading: userLoading } = useMe();
  const isOwner = user?.account?.role === "owner";
  const { data: billing, isLoading: billingLoading } = useBilling();
  const [txOffset, setTxOffset] = useState(0);
  const { data: txData, isLoading: txLoading } = useTransactions(TX_PAGE_SIZE, txOffset);
  const { data: allTxData } = useAllTransactions();

  // Spend limit form state
  const [spendLimit, setSpendLimit] = useState("");

  const isLoading = userLoading || billingLoading;

  // Hit the portal endpoint on click rather than on mount. This keeps the
  // session URL fresh (Hyperline URLs are short-lived) and avoids calling
  // the upstream unnecessarily on page loads.
  const portalMutation = useMutation({
    mutationFn: fetchBillingPortal,
    onSuccess: (data) => {
      // Full-page redirect — Hyperline portal is not iframe-safe.
      window.location.href = data.url;
    },
    onError: (error: Error) => {
      toast.error(error.message || "Failed to open billing portal");
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
  const hyperlineLinked = !!billing?.hyperline_customer_id;
  const walletBalance = billing?.hyperline_wallet_balance ?? null;
  const walletCurrency = billing?.hyperline_wallet_currency ?? null;
  const pmStatus = billing?.payment_method_status ?? null;

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Billing</h2>
        <p className="text-muted-foreground">
          Manage your credits, payment methods, and spend limits
        </p>
      </div>

      {/* Payment method status banner — surfaced from Hyperline
          payment_method.errored / payment_method.expired webhooks. */}
      {pmStatus && (
        <Card className="border-destructive/50 bg-destructive/5">
          <CardContent className="flex items-start gap-3 py-4">
            <AlertTriangle className="mt-0.5 h-5 w-5 flex-none text-destructive" />
            <div className="flex-1 space-y-1">
              <p className="text-sm font-medium">
                {pmStatus === "expired"
                  ? "Your payment method has expired"
                  : "Your payment method is not working"}
              </p>
              <p className="text-sm text-muted-foreground">
                Update your card on the billing portal to avoid service
                interruption when your next top-up runs.
              </p>
            </div>
            {isOwner && hyperlineLinked && (
              <Button
                size="sm"
                variant="outline"
                disabled={portalMutation.isPending}
                onClick={() => portalMutation.mutate()}
              >
                {portalMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <ExternalLink className="mr-2 h-4 w-4" />
                )}
                Update payment method
              </Button>
            )}
          </CardContent>
        </Card>
      )}

      {/* Credits Balance */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="flex items-center gap-2">
                <Zap className="h-5 w-5" />
                Credits Balance
              </CardTitle>
              <CardDescription>
                Credits are consumed per API request. All features are available with credits.
              </CardDescription>
            </div>
            <Sheet>
              <SheetTrigger asChild>
                <Button variant="outline" size="sm">
                  <Info className="mr-2 h-4 w-4" />
                  How credits work
                </Button>
              </SheetTrigger>
              <SheetContent className="overflow-y-auto sm:max-w-xl">
                <SheetHeader className="pb-2">
                  <SheetTitle className="text-lg">How credits work</SheetTitle>
                  <SheetDescription>
                    Credits are consumed per API call. Cost depends on the endpoint and features you enable.
                  </SheetDescription>
                </SheetHeader>
                <div className="space-y-8 px-4 pt-4 pb-8">
                  {/* /scrape */}
                  <div className="space-y-3">
                    <h4 className="font-semibold">/scrape</h4>
                    <p className="text-sm text-muted-foreground leading-relaxed">
                      1 credit minimum per request. Base formats (<code className="text-xs bg-muted px-1.5 py-0.5 rounded">html</code>, <code className="text-xs bg-muted px-1.5 py-0.5 rounded">rawHtml</code>, <code className="text-xs bg-muted px-1.5 py-0.5 rounded">content</code>) are free.
                    </p>
                    <div className="rounded-lg border">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead className="py-2.5">Component</TableHead>
                            <TableHead className="py-2.5 text-right">Credits</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">
                              <span>Each feature: </span>
                              <span className="inline-flex flex-wrap gap-1 mt-1">
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">markdown</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">links</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">metadata</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">screenshot</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">schema</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">blocks</code>
                              </span>
                            </TableCell>
                            <TableCell className="py-2.5 text-right font-mono">+1 each</TableCell>
                          </TableRow>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">AI summary</TableCell>
                            <TableCell className="py-2.5 text-right font-mono">+5</TableCell>
                          </TableRow>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">AI extraction</TableCell>
                            <TableCell className="py-2.5 text-right font-mono">+5</TableCell>
                          </TableRow>
                        </TableBody>
                      </Table>
                    </div>
                  </div>

                  {/* /map */}
                  <div className="space-y-3">
                    <h4 className="font-semibold">/map</h4>
                    <p className="text-sm text-muted-foreground leading-relaxed">
                      Flat <span className="font-mono font-medium text-foreground">2 credits</span> per call, regardless of the number of URLs discovered.
                    </p>
                  </div>

                  {/* /crawl */}
                  <div className="space-y-3">
                    <h4 className="font-semibold">/crawl</h4>
                    <p className="text-sm text-muted-foreground leading-relaxed">
                      Credits are deducted at job completion. Cost per page depends on crawler type and enabled features.
                    </p>
                    <div className="rounded-lg border">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead className="py-2.5">Component</TableHead>
                            <TableHead className="py-2.5 text-right">Credits / page</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">HTTP mode (base)</TableCell>
                            <TableCell className="py-2.5 text-right font-mono">1</TableCell>
                          </TableRow>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">Browser / JS mode (base)</TableCell>
                            <TableCell className="py-2.5 text-right font-mono">2</TableCell>
                          </TableRow>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">
                              <span>Each feature: </span>
                              <span className="inline-flex flex-wrap gap-1 mt-1">
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">metadata</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">markdown</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">block_split</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">schema</code>
                                <code className="text-xs bg-muted px-1.5 py-0.5 rounded">custom_selectors</code>
                              </span>
                            </TableCell>
                            <TableCell className="py-2.5 text-right font-mono">+1 each</TableCell>
                          </TableRow>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">AI extraction</TableCell>
                            <TableCell className="py-2.5 text-right font-mono">+5</TableCell>
                          </TableRow>
                          <TableRow>
                            <TableCell className="py-2.5 text-sm">AI summary</TableCell>
                            <TableCell className="py-2.5 text-right font-mono">+5</TableCell>
                          </TableRow>
                        </TableBody>
                      </Table>
                    </div>
                    <p className="text-sm text-muted-foreground">
                      Formula: <code className="bg-muted px-1.5 py-0.5 rounded text-xs">total = pages_crawled x (base + feature_costs)</code>
                    </p>
                  </div>

                  <div className="rounded-lg border border-dashed p-4">
                    <p className="text-sm text-muted-foreground">
                      No feature restrictions. Everything is available — you only pay for what you use.
                    </p>
                  </div>
                </div>
              </SheetContent>
            </Sheet>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-baseline gap-2">
            <span className="text-4xl font-bold">
              {Number(creditsBalance).toLocaleString()}
            </span>
            <span className="text-muted-foreground">credits remaining</span>
          </div>
          {/* Hyperline wallet mirror: the account's cash wallet on the
              billing provider. Shown when the account is linked and the
              live balance read succeeded. */}
          {hyperlineLinked && walletBalance !== null && (
            <div className="flex items-center justify-between border-t pt-4 text-sm">
              <div className="text-muted-foreground">
                Wallet balance
                <span className="ml-2 text-xs">(on Hyperline)</span>
              </div>
              <div className="font-mono font-medium">
                {formatMoney(walletBalance, walletCurrency)}
              </div>
            </div>
          )}
        </CardContent>
      </Card>

      {/* Hyperline hosted portal — single entry point for card management,
          invoices, auto-recharge, and one-off top-ups. Owners only. */}
      {isOwner && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <CreditCard className="h-5 w-5" />
              Payment methods & invoices
            </CardTitle>
            <CardDescription>
              Manage your card, view and download invoices, configure
              auto-recharge, and buy credits on the hosted billing portal.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {hyperlineLinked ? (
              <Button
                onClick={() => portalMutation.mutate()}
                disabled={portalMutation.isPending}
              >
                {portalMutation.isPending ? (
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                ) : (
                  <ExternalLink className="mr-2 h-4 w-4" />
                )}
                Open billing portal
              </Button>
            ) : (
              <p className="text-sm text-muted-foreground">
                Your account isn&apos;t linked to the billing provider yet.
                Contact support if you need to purchase credits.
              </p>
            )}
          </CardContent>
        </Card>
      )}

      {/* Monthly Spend Limit — owner only. Kept local: no equivalent
          Hyperline primitive, and we want the hard stop to live in our
          ledger rather than round-tripping to the provider. */}
      {isOwner && (
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
      )}

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
                        {tx.description || "\u2014"}
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        <span
                          className={
                            tx.amount > 0
                              ? "text-green-600 dark:text-green-400"
                              : tx.amount < 0
                                ? "text-red-600 dark:text-red-400"
                                : "text-muted-foreground"
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
                    Showing {txOffset + 1}&ndash;{Math.min(txOffset + TX_PAGE_SIZE, txData.total)} of{" "}
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
