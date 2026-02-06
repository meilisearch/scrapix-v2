"use client";

import { useEffect, useState } from "react";
import { createClient } from "@/lib/supabase/client";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Check, Loader2 } from "lucide-react";

interface Account {
  id: string;
  name: string;
  tier: string;
  stripe_customer_id: string | null;
}

const plans = [
  {
    name: "Free",
    tier: "free",
    price: "$0",
    period: "forever",
    description: "For hobbyists and testing",
    features: [
      "1,000 pages / month",
      "500 MB bandwidth",
      "1 API key",
      "Community support",
    ],
    limits: { pages: 1000, bandwidth: 500 },
  },
  {
    name: "Starter",
    tier: "starter",
    price: "$29",
    period: "per month",
    description: "For small projects",
    features: [
      "10,000 pages / month",
      "5 GB bandwidth",
      "5 API keys",
      "Email support",
      "JavaScript rendering",
    ],
    limits: { pages: 10000, bandwidth: 5000 },
    popular: true,
  },
  {
    name: "Pro",
    tier: "pro",
    price: "$99",
    period: "per month",
    description: "For growing businesses",
    features: [
      "100,000 pages / month",
      "50 GB bandwidth",
      "Unlimited API keys",
      "Priority support",
      "JavaScript rendering",
      "Custom crawl schedules",
    ],
    limits: { pages: 100000, bandwidth: 50000 },
  },
  {
    name: "Enterprise",
    tier: "enterprise",
    price: "Custom",
    period: "",
    description: "For large-scale operations",
    features: [
      "Unlimited pages",
      "Unlimited bandwidth",
      "Unlimited API keys",
      "Dedicated support",
      "SLA guarantee",
      "Custom integrations",
      "On-premise deployment",
    ],
    limits: { pages: Infinity, bandwidth: Infinity },
  },
];

export default function BillingPage() {
  const [account, setAccount] = useState<Account | null>(null);
  const [loading, setLoading] = useState(true);
  const [upgrading, setUpgrading] = useState<string | null>(null);
  const supabase = createClient();

  // Mock usage data - in production this would come from ClickHouse
  const usage = {
    pages: 2350,
    bandwidth: 1200,
  };

  useEffect(() => {
    fetchAccount();
  }, []);

  const fetchAccount = async () => {
    const {
      data: { user },
    } = await supabase.auth.getUser();
    if (!user) return;

    const { data: membership } = await supabase
      .from("account_members")
      .select("account_id")
      .eq("user_id", user.id)
      .single();

    if (membership) {
      const { data: accountData } = await supabase
        .from("accounts")
        .select("*")
        .eq("id", membership.account_id)
        .single();

      if (accountData) {
        setAccount(accountData);
      }
    }
    setLoading(false);
  };

  const handleUpgrade = async (tier: string) => {
    // In production, this would redirect to Stripe Checkout
    setUpgrading(tier);

    // Simulate API call
    await new Promise((resolve) => setTimeout(resolve, 1500));

    // Update the account tier
    if (account) {
      await supabase.from("accounts").update({ tier }).eq("id", account.id);
      setAccount({ ...account, tier });
    }

    setUpgrading(null);
  };

  const currentPlan = plans.find((p) => p.tier === account?.tier) || plans[0];

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-2xl font-bold tracking-tight">Billing</h2>
        <p className="text-muted-foreground">
          Manage your subscription and view usage
        </p>
      </div>

      {/* Current Plan & Usage */}
      <div className="grid gap-6 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle>Current Plan</CardTitle>
            <CardDescription>
              You are currently on the {currentPlan.name} plan
            </CardDescription>
          </CardHeader>
          <CardContent>
            <div className="flex items-baseline gap-2">
              <span className="text-4xl font-bold">{currentPlan.price}</span>
              {currentPlan.period && (
                <span className="text-muted-foreground">
                  {currentPlan.period}
                </span>
              )}
            </div>
            <ul className="mt-4 space-y-2">
              {currentPlan.features.slice(0, 4).map((feature) => (
                <li key={feature} className="flex items-center gap-2 text-sm">
                  <Check className="h-4 w-4 text-green-500" />
                  {feature}
                </li>
              ))}
            </ul>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>This Month&apos;s Usage</CardTitle>
            <CardDescription>
              Your usage resets on the 1st of each month
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div>
              <div className="flex justify-between text-sm mb-1">
                <span>Pages Crawled</span>
                <span>
                  {usage.pages.toLocaleString()} /{" "}
                  {currentPlan.limits.pages === Infinity
                    ? "Unlimited"
                    : currentPlan.limits.pages.toLocaleString()}
                </span>
              </div>
              <div className="w-full bg-muted rounded-full h-2">
                <div
                  className="bg-primary h-2 rounded-full transition-all"
                  style={{
                    width:
                      currentPlan.limits.pages === Infinity
                        ? "0%"
                        : `${Math.min(
                            (usage.pages / currentPlan.limits.pages) * 100,
                            100
                          )}%`,
                  }}
                ></div>
              </div>
            </div>
            <div>
              <div className="flex justify-between text-sm mb-1">
                <span>Bandwidth</span>
                <span>
                  {(usage.bandwidth / 1000).toFixed(1)} GB /{" "}
                  {currentPlan.limits.bandwidth === Infinity
                    ? "Unlimited"
                    : `${currentPlan.limits.bandwidth / 1000} GB`}
                </span>
              </div>
              <div className="w-full bg-muted rounded-full h-2">
                <div
                  className="bg-primary h-2 rounded-full transition-all"
                  style={{
                    width:
                      currentPlan.limits.bandwidth === Infinity
                        ? "0%"
                        : `${Math.min(
                            (usage.bandwidth / currentPlan.limits.bandwidth) *
                              100,
                            100
                          )}%`,
                  }}
                ></div>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* Plans */}
      <div>
        <h3 className="text-lg font-semibold mb-4">Available Plans</h3>
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          {plans.map((plan) => (
            <Card
              key={plan.tier}
              className={
                plan.popular ? "border-primary shadow-md" : ""
              }
            >
              <CardHeader>
                <div className="flex items-center justify-between">
                  <CardTitle>{plan.name}</CardTitle>
                  {plan.popular && <Badge>Popular</Badge>}
                </div>
                <CardDescription>{plan.description}</CardDescription>
              </CardHeader>
              <CardContent>
                <div className="flex items-baseline gap-1 mb-4">
                  <span className="text-3xl font-bold">{plan.price}</span>
                  {plan.period && (
                    <span className="text-sm text-muted-foreground">
                      {plan.period}
                    </span>
                  )}
                </div>
                <ul className="space-y-2">
                  {plan.features.map((feature) => (
                    <li
                      key={feature}
                      className="flex items-center gap-2 text-sm"
                    >
                      <Check className="h-4 w-4 text-green-500 flex-shrink-0" />
                      <span>{feature}</span>
                    </li>
                  ))}
                </ul>
              </CardContent>
              <CardFooter>
                {account?.tier === plan.tier ? (
                  <Button className="w-full" disabled variant="outline">
                    Current Plan
                  </Button>
                ) : plan.tier === "enterprise" ? (
                  <Button className="w-full" variant="outline">
                    Contact Sales
                  </Button>
                ) : (
                  <Button
                    className="w-full"
                    onClick={() => handleUpgrade(plan.tier)}
                    disabled={upgrading !== null}
                  >
                    {upgrading === plan.tier ? (
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    ) : null}
                    {plans.findIndex((p) => p.tier === account?.tier) <
                    plans.findIndex((p) => p.tier === plan.tier)
                      ? "Upgrade"
                      : "Downgrade"}
                  </Button>
                )}
              </CardFooter>
            </Card>
          ))}
        </div>
      </div>
    </div>
  );
}
