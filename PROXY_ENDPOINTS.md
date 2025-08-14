# Scrapix Proxy Regional Endpoints

All proxy servers are deployed across multiple regions for global low-latency access.
Each proxy auto-starts when accessed and auto-stops when idle to save resources.

## Regional Endpoints

| Region | Location | Health Check | Proxy Endpoint | Status |
|--------|----------|--------------|----------------|--------|
| **iad** | Washington DC, USA | https://scrapix-proxy-iad.fly.dev/proxy-health | scrapix-proxy-iad.fly.dev:8080 | ✅ Deployed |
| **lax** | Los Angeles, USA | https://scrapix-proxy-lax.fly.dev/proxy-health | scrapix-proxy-lax.fly.dev:8080 | ✅ Deployed |
| **ord** | Chicago, USA | https://scrapix-proxy-ord.fly.dev/proxy-health | scrapix-proxy-ord.fly.dev:8080 | ✅ Deployed |
| **lhr** | London, UK | https://scrapix-proxy-lhr.fly.dev/proxy-health | scrapix-proxy-lhr.fly.dev:8080 | ✅ Deployed |
| **fra** | Frankfurt, Germany | https://scrapix-proxy-fra.fly.dev/proxy-health | scrapix-proxy-fra.fly.dev:8080 | ✅ Deployed |
| **ams** | Amsterdam, Netherlands | https://scrapix-proxy-ams.fly.dev/proxy-health | scrapix-proxy-ams.fly.dev:8080 | ✅ Deployed |
| **sin** | Singapore | https://scrapix-proxy-sin.fly.dev/proxy-health | scrapix-proxy-sin.fly.dev:8080 | ✅ Deployed |
| **syd** | Sydney, Australia | https://scrapix-proxy-syd.fly.dev/proxy-health | scrapix-proxy-syd.fly.dev:8080 | ✅ Deployed |
| **nrt** | Tokyo, Japan | https://scrapix-proxy-nrt.fly.dev/proxy-health | scrapix-proxy-nrt.fly.dev:8080 | ✅ Deployed |
| **gru** | São Paulo, Brazil | https://scrapix-proxy-gru.fly.dev/proxy-health | scrapix-proxy-gru.fly.dev:8080 | ✅ Deployed |

## Management Commands

### Deploy all regions
```bash
yarn deploy:proxy:regions
```

### Deploy specific region
```bash
./scripts/deploy-proxy-regional-apps.sh deploy iad
```

### Check status
```bash
yarn deploy:proxy:status
```

### Destroy all regional apps
```bash
yarn deploy:proxy:destroy
```

### Destroy specific region
```bash
./scripts/deploy-proxy-regional-apps.sh destroy iad
```

## Architecture

- Each regional app is a separate Fly.io application
- Automatic scaling configured (min: 0, max: 10 machines)
- Auto-start on incoming requests
- Auto-stop when idle to minimize costs
- HTTP management interface on port 80/443
- TCP proxy service on port 8080
- Health checks configured for both services

## Usage Examples

### Connect through regional proxy
```bash
# Using curl through a regional proxy
curl -x scrapix-proxy-iad.fly.dev:8080 https://example.com

# Using the proxy from code
const proxy = 'http://scrapix-proxy-lhr.fly.dev:8080';
```

### Health check
```bash
# Check if proxy is healthy
curl https://scrapix-proxy-sin.fly.dev/proxy-health
# Returns: {"region":"sin"}
```