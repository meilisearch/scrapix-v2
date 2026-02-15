import { NextRequest } from "next/server";

const BACKEND = process.env.SCRAPIX_API_URL || "http://localhost:8080";

async function proxy(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  const { path } = await params;
  const target = `${BACKEND}/${path.join("/")}${req.nextUrl.search}`;

  const headers: Record<string, string> = {
    "content-type": req.headers.get("content-type") || "application/json",
  };

  // Forward cookie header for session auth
  const cookie = req.headers.get("cookie");
  if (cookie) {
    headers["cookie"] = cookie;
  }

  // Forward API key header if present
  const apiKey = req.headers.get("x-api-key");
  if (apiKey) {
    headers["x-api-key"] = apiKey;
  }

  const res = await fetch(target, {
    method: req.method,
    headers,
    body: req.method !== "GET" && req.method !== "HEAD" ? await req.text() : undefined,
  });

  // Build response headers, forwarding set-cookie from backend
  const responseHeaders = new Headers({
    "content-type": res.headers.get("content-type") || "application/json",
  });

  // Forward all set-cookie headers
  const setCookies = res.headers.getSetCookie();
  for (const sc of setCookies) {
    responseHeaders.append("set-cookie", sc);
  }

  return new Response(res.body, {
    status: res.status,
    headers: responseHeaders,
  });
}

export const GET = proxy;
export const POST = proxy;
export const PUT = proxy;
export const DELETE = proxy;
export const PATCH = proxy;
