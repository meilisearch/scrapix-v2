import { useQuery } from "@tanstack/react-query";
import { getMe } from "./auth";
import { fetchApiKeys, fetchServiceHealth } from "./api";

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
