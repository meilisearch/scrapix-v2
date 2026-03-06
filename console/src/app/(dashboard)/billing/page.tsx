"use client";

import { useMe } from "@/lib/hooks";
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
import { Check } from "lucide-react";
import { Skeleton } from "@/components/ui/skeleton";

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
  },
];

export default function BillingPage() {
  const { data: user, isLoading } = useMe();

  const accountTier = user?.account?.tier || "free";
  const currentPlan = plans.find((p) => p.tier === accountTier) || plans[0];

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
          Manage your subscription and view usage
        </p>
      </div>

      {/* Current Plan */}
      <Card>
        <CardHeader>
          <CardTitle>Current Plan</CardTitle>
          <CardDescription>
            You are on the <span className="font-medium">{currentPlan.name}</span> plan
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
            {currentPlan.features.map((feature) => (
              <li key={feature} className="flex items-center gap-2 text-sm">
                <Check className="h-4 w-4 text-green-500" />
                {feature}
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>

      {/* Plans */}
      <div>
        <h3 className="text-lg font-semibold mb-4">Available Plans</h3>
        <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
          {plans.map((plan) => (
            <Card
              key={plan.tier}
              className={plan.popular ? "border-primary shadow-md" : ""}
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
                {accountTier === plan.tier ? (
                  <Button className="w-full" disabled variant="outline">
                    Current Plan
                  </Button>
                ) : plan.tier === "enterprise" ? (
                  <Button
                    className="w-full"
                    variant="outline"
                    onClick={() =>
                      window.open("mailto:billing@meilisearch.com", "_blank")
                    }
                  >
                    Contact Sales
                  </Button>
                ) : (
                  <Button className="w-full" disabled variant="outline">
                    Coming Soon
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
