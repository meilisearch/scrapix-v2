import { useAccountStore } from "./account-store";

const BASE = "/api/scrapix";

export interface AuthUser {
  id: string;
  email: string;
  full_name: string | null;
  email_verified?: boolean;
  notify_job_emails?: boolean;
  account: {
    id: string;
    name: string;
    tier: string;
    active: boolean;
    role: string;
    credits_balance: number;
  } | null;
}

export async function login(
  email: string,
  password: string
): Promise<AuthUser> {
  const res = await fetch(`${BASE}/auth/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password }),
    credentials: "include",
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: "Login failed" }));
    throw new Error(body.error || "Login failed");
  }
  return res.json();
}

export async function signup(
  email: string,
  password: string,
  full_name?: string
): Promise<AuthUser> {
  const res = await fetch(`${BASE}/auth/signup`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email, password, full_name }),
    credentials: "include",
  });
  if (!res.ok) {
    const body = await res.json().catch(() => ({ error: "Signup failed" }));
    throw new Error(body.error || "Signup failed");
  }
  return res.json();
}

export async function logout(): Promise<void> {
  await fetch(`${BASE}/auth/logout`, {
    method: "POST",
    credentials: "include",
  });
}

export async function getMe(): Promise<AuthUser> {
  const headers: Record<string, string> = {};
  const accountId = useAccountStore.getState().selectedAccountId;
  if (accountId) {
    headers["X-Account-Id"] = accountId;
  }
  const res = await fetch(`${BASE}/auth/me`, {
    credentials: "include",
    headers,
  });
  if (!res.ok) {
    throw new Error("Not authenticated");
  }
  return res.json();
}
