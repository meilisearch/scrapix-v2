# Fly.io Deployment Runbook

Scrapix runs as six Fly apps. Redpanda is a durable singleton; every other app
scale-to-zeros when idle. CI (`.github/workflows/deploy-fly.yml`) builds images
in GitHub Actions, pushes them to GHCR, and deploys via `flyctl deploy --image`.

## Apps

| App | Type | Scaling | Auto-suspend |
|---|---|---|---|
| `scrapix-redpanda` | Kafka broker (port 9092, `*.internal` only) | singleton | no (data loss on suspend) |
| `scrapix-api` | HTTP API | auto 1–N on HTTP requests | yes |
| `scrapix-console` | Next.js | auto 1–N on HTTP requests | yes |
| `scrapix-frontier` | Kafka consumer | **singleton** (bloom filter state) | yes, via wake ping |
| `scrapix-worker-crawler` | Kafka consumer | horizontal, `flyctl scale count N` | yes, via wake ping |
| `scrapix-worker-content` | Kafka consumer | horizontal, `flyctl scale count N` | yes, via wake ping |

## Scale-to-zero mechanics

Workers don't receive HTTP traffic, so Fly can't auto-start them normally. The
API binary, on every `POST /crawl`, fires a fire-and-forget TCP connect to each
worker's `WAKE_PORT` (8081) via Fly's private `*.internal` mesh — that connect
triggers Fly's proxy to autostart the suspended machine. Each worker exits
cleanly after `IDLE_EXIT_MINUTES` with no Kafka messages (default 10 min), at
which point Fly suspends it to zero cost.

Environment variables you can tune:
- `WORKER_WAKE_HOSTS` on the API: comma-separated `host:port` list. Unset = no fan-out (local/dev).
- `IDLE_EXIT_MINUTES` on workers: `0` disables the watchdog (for always-on workloads).
- `WAKE_PORT` on workers: default `8081`.

## First-time setup

Set these env vars before running the commands below:

```bash
export FLY_ORG=meilisearch           # confirm with `flyctl orgs list`
export FLY_REGION=cdg
```

### 1. Redpanda

```bash
flyctl apps create scrapix-redpanda --org "$FLY_ORG"
flyctl volumes create redpanda_data --app scrapix-redpanda --region "$FLY_REGION" --size 10
flyctl deploy --config deploy/fly/redpanda/fly.toml --app scrapix-redpanda
# Wait for healthy:
flyctl status --app scrapix-redpanda
```

Create the seven Kafka topics with partition counts that match `docker-compose.yml`:

```bash
./deploy/fly/create-topics.sh
```

### 2. Create remaining Fly apps

```bash
for app in scrapix-api scrapix-console scrapix-frontier scrapix-worker-crawler scrapix-worker-content; do
  flyctl apps create "$app" --org "$FLY_ORG"
done
```

### 3. Set secrets

Each app needs its own copy — secrets don't share across apps.

```bash
# Shared secrets for every Rust service (api + 3 workers):
for app in scrapix-api scrapix-frontier scrapix-worker-crawler scrapix-worker-content; do
  flyctl secrets set \
    KAFKA_BROKERS=scrapix-redpanda.internal:9092 \
    MEILISEARCH_URL=... \
    MEILISEARCH_API_KEY=... \
    REDIS_URL=... \
    CLICKHOUSE_URL=https://ka2htxje0a.eu-central-1.aws.clickhouse.cloud:8443 \
    CLICKHOUSE_USER=... \
    CLICKHOUSE_PASSWORD=... \
    CLICKHOUSE_DATABASE=scrapix \
    --app "$app"
done

# API-only secrets:
flyctl secrets set \
  DATABASE_URL=... \
  JWT_SECRET=... \
  OPENAI_API_KEY=... \
  STRIPE_SECRET_KEY=... \
  --app scrapix-api

# Console-only:
flyctl secrets set \
  NEXT_PUBLIC_SCRAPIX_API_URL=https://api.scrapix.meilisearch.com \
  NEXT_PUBLIC_STRIPE_PUBLISHABLE_KEY=... \
  --app scrapix-console
```

### 4. First deploy

Push to `main` — CI builds and deploys everything. Or deploy manually from a
locally-pulled GHCR image:

```bash
flyctl deploy --config deploy/fly/api/fly.toml \
  --image ghcr.io/<owner>/scrapix-api:main \
  --app scrapix-api
```

### 5. Custom domains

```bash
flyctl certs add api.scrapix.meilisearch.com --app scrapix-api
flyctl certs add scrapix.meilisearch.dev --app scrapix-api
flyctl certs add scrapix.meilisearch.com --app scrapix-console
# flyctl prints the DNS records to add (AAAA + A + acme-challenge CNAME).
```

Add those records at the DNS registrar with a short TTL (60 s) while Heroku
still serves the production domains. When you flip the A/AAAA records to Fly,
traffic cuts over; flip them back to Heroku for rollback.

## Scaling

```bash
# Scale out for heavy crawling:
flyctl scale count 6 --app scrapix-worker-crawler   # max 12 (partition count of scrapix.urls.frontier)
flyctl scale count 3 --app scrapix-worker-content   # max 6 (partition count of scrapix.pages.raw)

# Scale back to zero:
flyctl scale count 0 --app scrapix-worker-crawler
```

Kafka consumer groups rebalance automatically when machines join/leave. Offset
commits are manual (`enable.auto.commit = false`), so in-flight messages either
finish and commit, or get reprocessed by another machine after the 30 s
`kill_timeout`.

**Do not** scale `scrapix-frontier` beyond 1 — the bloom filter is per-process
state.

## Debugging

```bash
flyctl logs --app scrapix-worker-crawler
flyctl ssh console --app scrapix-api
flyctl status --app scrapix-api
flyctl machine status <id> --app scrapix-worker-crawler
```

## Rollback

```bash
flyctl releases --app scrapix-api
flyctl deploy --image <previous-sha-image> --app scrapix-api
```

Or flip DNS back to Heroku (which stays warm through the cutover window).
