import { NextRequest } from "next/server";

const BACKEND = process.env.SCRAPIX_API_URL || "http://localhost:8080";

async function proxy(req: NextRequest, { params }: { params: Promise<{ path: string[] }> }) {
  const { path } = await params;
  const target = `${BACKEND}/${path.join("/")}${req.nextUrl.search}`;

  const res = await fetch(target, {
    method: req.method,
    headers: {
      "content-type": req.headers.get("content-type") || "application/json",
    },
    body: req.method !== "GET" && req.method !== "HEAD" ? await req.text() : undefined,
  });

  return new Response(res.body, {
    status: res.status,
    headers: {
      "content-type": res.headers.get("content-type") || "application/json",
    },
  });
}

export const GET = proxy;
export const POST = proxy;
export const PUT = proxy;
export const DELETE = proxy;
export const PATCH = proxy;
