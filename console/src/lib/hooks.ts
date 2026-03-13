import { useQuery } from "@tanstack/react-query";
import { getMe } from "./auth";
import { fetchApiKeys, fetchServiceHealth, fetchBilling, fetchTransactions } from "./api";

export function useMe() {
  return useQuery({
    queryKey: ["me"],
    queryFn: getMe,
    staleTime: 60_000,
    retry: false,
  });
}

export function useApiKeys() {
  return useQuery({
    queryKey: ["api-keys"],
    queryFn: fetchApiKeys,
  });
}

export function useServiceHealth() {
  return useQuery({
    queryKey: ["service-health"],
    queryFn: fetchServiceHealth,
    refetchInterval: 10_000,
  });
}

export function useBilling() {
  return useQuery({
    queryKey: ["billing"],
    queryFn: fetchBilling,
  });
}

export function useTransactions(limit: number = 50, offset: number = 0) {
  return useQuery({
    queryKey: ["transactions", limit, offset],
    queryFn: () => fetchTransactions(limit, offset),
  });
}

export function useAllTransactions() {
  return useQuery({
    queryKey: ["transactions", "all"],
    queryFn: async () => {
      const first = await fetchTransactions(1, 0);
      if (first.total <= 0) return { transactions: [], total: 0 };
      const all = await fetchTransactions(first.total, 0);
      return all;
    },
  });
}
