# Fly.io Deployment Guide

## Quick Deploy

Use the automated deployment script:
```bash
./deploy.sh
```

## Manual Deployment Commands

### 1. Initial Setup

Create the app (first time only):
```bash
flyctl apps create scrapix --org personal
```

### 2. Set Required Secrets

#### Required: Redis URL for job queue
```bash
flyctl secrets set REDIS_URL="redis://default:your-password@your-redis-host:6379" --app scrapix
```

#### Optional: OpenAI API Key for AI features
```bash
flyctl secrets set OPENAI_API_KEY="sk-..." --app scrapix
```

### 3. Deploy the Application
```bash
flyctl deploy --config fly.toml --wait-timeout 600
```

### 4. Verify Deployment
```bash
flyctl status --app scrapix
flyctl logs --app scrapix
```

## Redis Setup Options

### Option 1: Upstash Redis (Recommended)
1. Create account at https://upstash.com
2. Create Redis database
3. Copy the Redis URL from dashboard
4. Set as secret: `flyctl secrets set REDIS_URL="..." --app scrapix`

### Option 2: Fly Redis (Deprecated but works)
```bash
flyctl redis create
flyctl redis list
flyctl redis status <redis-name>
```

## Monitoring & Management

### View logs
```bash
flyctl logs --app scrapix
```

### SSH into container
```bash
flyctl ssh console --app scrapix
```

### Scale application
```bash
# Manual scaling
flyctl scale count 2 --app scrapix

# Autoscaling (machines will auto-suspend when idle)
flyctl autoscale set min=0 max=3 --app scrapix
```

### Check metrics
```bash
flyctl monitor --app scrapix
```

## Troubleshooting

### Build failures
- Check Docker build locally: `docker build .`
- Ensure all dependencies are included in package.json
- Verify TypeScript builds successfully: `yarn build`

### Runtime errors
- Check logs: `flyctl logs --app scrapix`
- Verify environment variables: `flyctl secrets list --app scrapix`
- Test health endpoint: `curl https://scrapix.fly.dev/health`

### Memory issues
- Scale up VM: `flyctl scale vm shared-cpu-2x --app scrapix`
- Add swap: `flyctl scale memory 2048 --app scrapix`

## Configuration Notes

The `fly.toml` configuration includes:
- Auto-stop/start machines to save costs
- Health checks on `/health` endpoint
- HTTPS enforcement
- Puppeteer with pre-installed Chrome
- 1GB RAM (minimum for Puppeteer)

Your API will be available at: `https://scrapix.fly.dev`