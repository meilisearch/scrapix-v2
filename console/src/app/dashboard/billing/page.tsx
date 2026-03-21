"use client";

import { useState, useMemo, useCallback } from "react";
import { useBilling, useTransactions, useAllTransactions, useMe, usePaymentMethods, useInvoices } from "@/lib/hooks";
import {
  topupCredits,
  updateAutoTopup,
  updateSpendLimit,
  createSetupIntent,
  deletePaymentMethod,
  setDefaultPaymentMethod,
  purchaseCredits,
} from "@/lib/api";
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
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
  Plus,
  Trash2,
  Star,
  Loader2,
  Receipt,
  ExternalLink,
  FileDown,
  Info,
} from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { formatDistanceToNow, format, parseISO, eachDayOfInterval, startOfDay } from "date-fns";
import dynamic from "next/dynamic";
import { loadStripe } from "@stripe/stripe-js";
import {
  Elements,
  PaymentElement,
  useStripe,
  useElements,
} from "@stripe/react-stripe-js";

const DailyCostChart = dynamic(() => import("./daily-cost-chart"), {
  ssr: false,
});

const stripePromise = process.env.NEXT_PUBLIC_STRIPE_PUBLISHABLE_KEY
  ? loadStripe(process.env.NEXT_PUBLIC_STRIPE_PUBLISHABLE_KEY)
  : null;

const PRICING_TIERS = [
  { upTo: 999, rate: 0.01, per1k: "$10" },
  { upTo: 4_999, rate: 0.008, per1k: "$8" },
  { upTo: 9_999, rate: 0.007, per1k: "$7" },
  { upTo: null as number | null, rate: 0.005, per1k: "$5" },
];

const QUICK_AMOUNTS = [1_000, 5_000, 10_000, 50_000];

function calculatePrice(credits: number): number {
  if (credits >= 10_000) return credits * 0.005;
  if (credits >= 5_000) return credits * 0.007;
  if (credits >= 1_000) return credits * 0.008;
  return credits * 0.01;
}

function getActiveTier(credits: number): number {
  if (credits >= 10_000) return 3;
  if (credits >= 5_000) return 2;
  if (credits >= 1_000) return 1;
  return 0;
}

const TX_PAGE_SIZE = 5;

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
      return "Credit Purchase";
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

function cardBrandName(brand: string | null) {
  if (!brand) return "Card";
  const names: Record<string, string> = {
    visa: "Visa",
    mastercard: "Mastercard",
    amex: "Amex",
    discover: "Discover",
    diners: "Diners",
    jcb: "JCB",
    unionpay: "UnionPay",
  };
  return names[brand] ?? brand;
}

// ============================================================================
// Add Card Form (rendered inside <Elements>)
// ============================================================================

function AddCardForm({ onSuccess, onCancel }: { onSuccess: () => void; onCancel: () => void }) {
  const stripe = useStripe();
  const elements = useElements();
  const [loading, setLoading] = useState(false);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!stripe || !elements) return;

    setLoading(true);
    const { error } = await stripe.confirmSetup({
      elements,
      confirmParams: {
        return_url: window.location.href,
      },
      redirect: "if_required",
    });

    setLoading(false);
    if (error) {
      toast.error(error.message ?? "Failed to save card");
    } else {
      toast.success("Card saved successfully");
      onSuccess();
    }
  };

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <PaymentElement />
      <div className="flex gap-2">
        <Button type="submit" disabled={!stripe || loading}>
          {loading && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
          Save Card
        </Button>
        <Button type="button" variant="ghost" onClick={onCancel} disabled={loading}>
          Cancel
        </Button>
      </div>
    </form>
  );
}

// ============================================================================
// Main Billing Page
// ============================================================================

export default function BillingPage() {
  const queryClient = useQueryClient();
  const { data: user, isLoading: userLoading } = useMe();
  const { data: billing, isLoading: billingLoading } = useBilling();
  const { data: paymentMethods, isLoading: pmLoading } = usePaymentMethods();
  const { data: invoices, isLoading: invoicesLoading } = useInvoices();
  const [txOffset, setTxOffset] = useState(0);
  const { data: txData, isLoading: txLoading } = useTransactions(TX_PAGE_SIZE, txOffset);
  const { data: allTxData } = useAllTransactions();

  // Add card form state
  const [showAddCard, setShowAddCard] = useState(false);
  const [setupClientSecret, setSetupClientSecret] = useState<string | null>(null);

  // Purchase state
  const [purchaseAmount, setPurchaseAmount] = useState("1000");
  const [purchasingPack, setPurchasingPack] = useState<number | null>(null);
  const [showPurchaseConfirm, setShowPurchaseConfirm] = useState(false);

  // Auto top-up form state
  const [autoTopupAmount, setAutoTopupAmount] = useState("");
  const [autoTopupThreshold, setAutoTopupThreshold] = useState("");

  // Spend limit form state
  const [spendLimit, setSpendLimit] = useState("");

  const isLoading = userLoading || billingLoading;
  const hasStripe = !!stripePromise;
  const hasPaymentMethod = (paymentMethods?.length ?? 0) > 0;

  const invalidateBilling = useCallback(() => {
    queryClient.invalidateQueries({ queryKey: ["billing"] });
    queryClient.invalidateQueries({ queryKey: ["transactions"] });
    queryClient.invalidateQueries({ queryKey: ["me"] });
    queryClient.invalidateQueries({ queryKey: ["payment-methods"] });
  }, [queryClient]);

  // Start adding a card
  const handleAddCard = async () => {
    try {
      const { client_secret } = await createSetupIntent();
      setSetupClientSecret(client_secret);
      setShowAddCard(true);
    } catch (e) {
      toast.error(e instanceof Error ? e.message : "Failed to start card setup");
    }
  };

  // Purchase credits via Stripe
  const purchaseMutation = useMutation({
    mutationFn: async (credits: number) => {
      const result = await purchaseCredits(credits);
      if (result.status === "requires_action" && result.client_secret) {
        // Handle 3D Secure
        const stripe = await stripePromise;
        if (!stripe) throw new Error("Stripe not loaded");
        const { error } = await stripe.confirmPayment({
          clientSecret: result.client_secret,
          confirmParams: { return_url: window.location.href },
          redirect: "if_required",
        });
        if (error) throw new Error(error.message ?? "Payment authentication failed");
        return { ...result, status: "succeeded" as const, message: `${credits.toLocaleString()} credits added to your account` };
      }
      return result;
    },
    onSuccess: (data) => {
      toast.success(data.message);
      invalidateBilling();
      setPurchasingPack(null);
    },
    onError: (error: Error) => {
      toast.error(error.message);
      setPurchasingPack(null);
    },
  });

  // Fallback topup (no Stripe, for dev/free)
  const topupMutation = useMutation({
    mutationFn: (amount: number) => topupCredits(amount),
    onSuccess: (data) => {
      toast.success(data.message);
      invalidateBilling();
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const deletePmMutation = useMutation({
    mutationFn: (id: string) => deletePaymentMethod(id),
    onSuccess: () => {
      toast.success("Card removed");
      queryClient.invalidateQueries({ queryKey: ["payment-methods"] });
    },
    onError: (error: Error) => {
      toast.error(error.message);
    },
  });

  const setDefaultPmMutation = useMutation({
    mutationFn: (id: string) => setDefaultPaymentMethod(id),
    onSuccess: () => {
      toast.success("Default card updated");
      queryClient.invalidateQueries({ queryKey: ["payment-methods"] });
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

  const handlePurchase = (credits: number) => {
    if (hasStripe && hasPaymentMethod) {
      setPurchasingPack(credits);
      purchaseMutation.mutate(credits);
    } else if (hasStripe && !hasPaymentMethod) {
      toast.error("Please add a payment method first");
    } else {
      // Fallback: free topup (dev mode)
      topupMutation.mutate(credits);
    }
  };

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Billing</h2>
        <p className="text-muted-foreground">
          Manage your credits, payment methods, and spend limits
        </p>
      </div>

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
                <div className="space-y-8 px-1 pt-4 pb-8">
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
        <CardContent>
          <div className="flex items-baseline gap-2">
            <span className="text-4xl font-bold">
              {Number(creditsBalance).toLocaleString()}
            </span>
            <span className="text-muted-foreground">credits remaining</span>
          </div>
        </CardContent>
      </Card>

      {/* Payment Methods */}
      {hasStripe && (
        <Card>
          <CardHeader>
            <div className="flex items-center justify-between">
              <div>
                <CardTitle className="flex items-center gap-2">
                  <CreditCard className="h-5 w-5" />
                  Payment Methods
                </CardTitle>
                <CardDescription>
                  Manage your cards for credit purchases and auto top-up
                </CardDescription>
              </div>
              {!showAddCard && (
                <Button variant="outline" size="sm" onClick={handleAddCard}>
                  <Plus className="mr-2 h-4 w-4" />
                  Add Card
                </Button>
              )}
            </div>
          </CardHeader>
          <CardContent>
            {showAddCard && setupClientSecret && stripePromise ? (
              <Elements
                stripe={stripePromise}
                options={{
                  clientSecret: setupClientSecret,
                  appearance: {
                    theme: "night",
                    variables: {
                      colorPrimary: "#6366f1",
                      colorBackground: "hsl(var(--card))",
                      colorText: "hsl(var(--card-foreground))",
                      colorDanger: "hsl(var(--destructive))",
                      colorTextSecondary: "hsl(var(--muted-foreground))",
                      borderRadius: "8px",
                      fontFamily: "var(--font-geist-sans), system-ui, sans-serif",
                    },
                    rules: {
                      ".Input": {
                        backgroundColor: "hsl(var(--input))",
                        borderColor: "hsl(var(--border))",
                        color: "hsl(var(--foreground))",
                      },
                      ".Input:focus": {
                        borderColor: "hsl(var(--ring))",
                        boxShadow: "0 0 0 3px hsl(var(--ring) / 0.5)",
                      },
                      ".Label": {
                        color: "hsl(var(--foreground))",
                      },
                      ".Tab": {
                        backgroundColor: "hsl(var(--muted))",
                        borderColor: "hsl(var(--border))",
                        color: "hsl(var(--foreground))",
                      },
                      ".Tab--selected": {
                        backgroundColor: "hsl(var(--background))",
                        borderColor: "hsl(var(--ring))",
                        color: "hsl(var(--foreground))",
                      },
                      ".Block": {
                        backgroundColor: "hsl(var(--muted))",
                        borderColor: "hsl(var(--border))",
                      },
                    },
                  },
                }}
              >
                <AddCardForm
                  onSuccess={() => {
                    setShowAddCard(false);
                    setSetupClientSecret(null);
                    queryClient.invalidateQueries({ queryKey: ["payment-methods"] });
                  }}
                  onCancel={() => {
                    setShowAddCard(false);
                    setSetupClientSecret(null);
                  }}
                />
              </Elements>
            ) : pmLoading ? (
              <div className="space-y-2">
                <Skeleton className="h-12 w-full" />
                <Skeleton className="h-12 w-full" />
              </div>
            ) : !paymentMethods?.length ? (
              <p className="text-sm text-muted-foreground py-4 text-center">
                No payment methods yet. Add a card to purchase credits.
              </p>
            ) : (
              <div className="space-y-2">
                {paymentMethods.map((pm) => (
                  <div
                    key={pm.id}
                    className="flex items-center justify-between rounded-lg border p-3"
                  >
                    <div className="flex items-center gap-3">
                      <CreditCard className="h-5 w-5 text-muted-foreground" />
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium">
                            {cardBrandName(pm.brand)} ending in {pm.last4}
                          </span>
                          {pm.is_default && (
                            <Badge variant="secondary" className="text-xs">
                              Default
                            </Badge>
                          )}
                        </div>
                        <span className="text-xs text-muted-foreground">
                          Expires {pm.exp_month}/{pm.exp_year}
                        </span>
                      </div>
                    </div>
                    <div className="flex items-center gap-1">
                      {!pm.is_default && (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => setDefaultPmMutation.mutate(pm.id)}
                          disabled={setDefaultPmMutation.isPending}
                          title="Set as default"
                        >
                          <Star className="h-4 w-4" />
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => deletePmMutation.mutate(pm.id)}
                        disabled={deletePmMutation.isPending}
                        title="Remove card"
                      >
                        <Trash2 className="h-4 w-4 text-destructive" />
                      </Button>
                    </div>
                  </div>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {/* Buy Credits */}
      <Card>
        <CardHeader>
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="flex items-center gap-2">
                <CreditCard className="h-5 w-5" />
                {hasStripe ? "Buy Credits" : "Add Credits"}
              </CardTitle>
              <CardDescription>
                {hasStripe
                  ? "Purchase credits with your saved payment method"
                  : "Add credits to your account"}
              </CardDescription>
            </div>
            <Sheet>
              <SheetTrigger asChild>
                <Button variant="outline" size="sm">
                  <Info className="mr-2 h-4 w-4" />
                  Volume pricing
                </Button>
              </SheetTrigger>
              <SheetContent className="overflow-y-auto sm:max-w-md">
                <SheetHeader className="pb-2">
                  <SheetTitle className="text-lg">Volume pricing</SheetTitle>
                  <SheetDescription>
                    The more credits you buy, the less you pay per credit.
                  </SheetDescription>
                </SheetHeader>
                <div className="pt-4 pb-8">
                  <div className="rounded-lg border">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <TableHead className="py-2.5">Volume</TableHead>
                          <TableHead className="py-2.5 text-right">Per credit</TableHead>
                          <TableHead className="py-2.5 text-right">Per 1K</TableHead>
                        </TableRow>
                      </TableHeader>
                      <TableBody>
                        {PRICING_TIERS.map((tier, i) => (
                          <TableRow key={i}>
                            <TableCell className="py-2.5 text-sm">
                              {tier.upTo ? `1 – ${tier.upTo.toLocaleString()}` : "10,000+"}
                            </TableCell>
                            <TableCell className="py-2.5 text-right font-mono text-sm">
                              ${tier.rate.toFixed(3)}
                            </TableCell>
                            <TableCell className="py-2.5 text-right font-mono text-sm">
                              {tier.per1k}
                            </TableCell>
                          </TableRow>
                        ))}
                      </TableBody>
                    </Table>
                  </div>
                  <p className="mt-4 text-sm text-muted-foreground">
                    Volume-based pricing: the entire purchase is priced at the tier rate for the total quantity.
                  </p>
                </div>
              </SheetContent>
            </Sheet>
          </div>
        </CardHeader>
        <CardContent className="space-y-5">
          {/* Preset amounts + custom */}
          <div className="grid grid-cols-5 gap-2">
            {QUICK_AMOUNTS.map((amt) => {
              const price = calculatePrice(amt);
              const isSelected = purchaseAmount === String(amt);
              return (
                <button
                  key={amt}
                  type="button"
                  className={cn(
                    "flex flex-col items-center gap-0.5 rounded-lg border p-3 transition-colors",
                    isSelected
                      ? "border-primary bg-primary/5 ring-1 ring-primary"
                      : "border-border hover:border-muted-foreground/30"
                  )}
                  onClick={() => setPurchaseAmount(String(amt))}
                >
                  <span className="text-sm font-bold">
                    {amt >= 1000 ? `${(amt / 1000).toLocaleString()}K` : amt.toLocaleString()}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    ${price.toFixed(0)}
                  </span>
                </button>
              );
            })}
            <button
              type="button"
              className={cn(
                "flex flex-col items-center gap-0.5 rounded-lg border p-3 transition-colors",
                !QUICK_AMOUNTS.includes(parseInt(purchaseAmount))
                  ? "border-primary bg-primary/5 ring-1 ring-primary"
                  : "border-border hover:border-muted-foreground/30"
              )}
              onClick={() => {
                setPurchaseAmount("");
                document.getElementById("credit-amount")?.focus();
              }}
            >
              <span className="text-sm font-bold">Custom</span>
              <span className="text-xs text-muted-foreground">Any amount</span>
            </button>
          </div>

          {/* Custom amount input (always visible but highlighted when Custom is selected) */}
          {!QUICK_AMOUNTS.includes(parseInt(purchaseAmount)) && (
            <div>
              <Label htmlFor="credit-amount" className="mb-2">
                Custom amount
              </Label>
              <Input
                id="credit-amount"
                type="number"
                min={100}
                step={100}
                value={purchaseAmount}
                onChange={(e) => setPurchaseAmount(e.target.value)}
                placeholder="Enter number of credits (min 100)"
              />
            </div>
          )}

          {/* Order summary */}
          {(() => {
            const amt = parseInt(purchaseAmount) || 0;
            if (amt < 100) return null;
            const price = calculatePrice(amt);
            const per1k = (price / amt) * 1000;
            return (
              <div className="rounded-lg border bg-muted/30 p-4">
                <div className="flex items-center justify-between">
                  <div className="space-y-1">
                    <div className="text-sm text-muted-foreground">
                      {amt.toLocaleString()} credits × ${(price / amt).toFixed(3)}/credit
                    </div>
                    <div className="text-xs text-muted-foreground">
                      ${per1k.toFixed(2)} per 1,000 credits
                    </div>
                  </div>
                  <div className="text-right">
                    <div className="text-2xl font-bold">${price.toFixed(2)}</div>
                  </div>
                </div>
                <div className="mt-4">
                  <Button
                    className="w-full"
                    disabled={
                      purchaseMutation.isPending ||
                      topupMutation.isPending ||
                      (hasStripe && !hasPaymentMethod)
                    }
                    onClick={() => setShowPurchaseConfirm(true)}
                  >
                    {purchaseMutation.isPending ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : null}
                    {purchaseMutation.isPending
                      ? "Processing..."
                      : `Buy ${amt.toLocaleString()} credits for $${price.toFixed(2)}`}
                  </Button>
                </div>
              </div>
            );
          })()}

          {hasStripe && !hasPaymentMethod && (
            <p className="text-sm text-muted-foreground">
              Add a payment method above to purchase credits.
            </p>
          )}
        </CardContent>
      </Card>

      {/* Purchase confirmation dialog */}
      <Dialog open={showPurchaseConfirm} onOpenChange={setShowPurchaseConfirm}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Confirm purchase</DialogTitle>
            <DialogDescription>
              You are about to purchase credits. Your default card will be charged.
            </DialogDescription>
          </DialogHeader>
          {(() => {
            const amt = parseInt(purchaseAmount) || 0;
            const price = calculatePrice(amt);
            return (
              <div className="space-y-3 py-2">
                <div className="flex justify-between text-sm">
                  <span className="text-muted-foreground">Credits</span>
                  <span className="font-mono font-medium">{amt.toLocaleString()}</span>
                </div>
                <div className="flex justify-between text-sm">
                  <span className="text-muted-foreground">Rate</span>
                  <span className="font-mono font-medium">${(price / amt).toFixed(3)} / credit</span>
                </div>
                <div className="border-t pt-3 flex justify-between">
                  <span className="font-medium">Total</span>
                  <span className="text-lg font-bold">${price.toFixed(2)}</span>
                </div>
              </div>
            );
          })()}
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowPurchaseConfirm(false)}>
              Cancel
            </Button>
            <Button
              disabled={purchaseMutation.isPending}
              onClick={() => {
                setShowPurchaseConfirm(false);
                handlePurchase(parseInt(purchaseAmount) || 0);
              }}
            >
              {purchaseMutation.isPending ? (
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              ) : null}
              Confirm purchase
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Auto Top-up */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <RefreshCw className="h-5 w-5" />
            Auto Top-up
          </CardTitle>
          <CardDescription>
            Automatically add credits when your balance drops below a threshold
            {hasStripe && hasPaymentMethod && " — charges your default card"}
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
                        {tx.description || "\u2014"}
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
                    Showing {txOffset + 1}\u2013{Math.min(txOffset + TX_PAGE_SIZE, txData.total)} of{" "}
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

      {/* Invoices */}
      {hasStripe && (
        <Card>
          <CardHeader>
            <CardTitle className="flex items-center gap-2">
              <Receipt className="h-5 w-5" />
              Invoices
            </CardTitle>
            <CardDescription>
              Download invoices and receipts
            </CardDescription>
          </CardHeader>
          <CardContent>
            {invoicesLoading ? (
              <div className="space-y-2">
                {Array.from({ length: 3 }).map((_, i) => (
                  <Skeleton key={i} className="h-10 w-full" />
                ))}
              </div>
            ) : !invoices?.length ? (
              <p className="text-sm text-muted-foreground py-4 text-center">
                No invoices yet
              </p>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Invoice</TableHead>
                    <TableHead>Date</TableHead>
                    <TableHead>Description</TableHead>
                    <TableHead className="text-right">Amount</TableHead>
                    <TableHead className="text-right">Credits</TableHead>
                    <TableHead className="text-right">PDF</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {invoices.map((inv) => (
                    <TableRow key={inv.id}>
                      <TableCell className="text-sm font-mono">
                        {inv.number ?? "—"}
                      </TableCell>
                      <TableCell className="text-sm text-muted-foreground">
                        {format(new Date(inv.created_at), "MMM d, yyyy")}
                      </TableCell>
                      <TableCell className="text-sm">
                        {inv.description || "Credit purchase"}
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        ${(inv.amount_cents / 100).toFixed(2)}
                      </TableCell>
                      <TableCell className="text-right font-mono text-sm">
                        {inv.credits?.toLocaleString() ?? "—"}
                      </TableCell>
                      <TableCell className="text-right">
                        <div className="flex items-center justify-end gap-1">
                          {inv.invoice_pdf && (
                            <a
                              href={inv.invoice_pdf}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
                              title="Download PDF"
                            >
                              <FileDown className="h-3.5 w-3.5" />
                            </a>
                          )}
                          {inv.hosted_invoice_url && (
                            <a
                              href={inv.hosted_invoice_url}
                              target="_blank"
                              rel="noopener noreferrer"
                              className="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
                              title="View invoice"
                            >
                              <ExternalLink className="h-3.5 w-3.5" />
                            </a>
                          )}
                        </div>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>
      )}
    </div>
  );
}
