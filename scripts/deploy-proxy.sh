#!/bin/bash

# Fly.io Deployment Script for Scrapix Proxy
# Usage: ./scripts/deploy-proxy.sh [staging|production]

set -e

ENVIRONMENT=${1:-production}
APP_NAME="scrapix-proxy"

if [ "$ENVIRONMENT" == "staging" ]; then
    APP_NAME="scrapix-proxy-staging"
fi

echo "🚀 Deploying Scrapix Proxy to Fly.io ($ENVIRONMENT)..."

# Check if fly CLI is installed
if ! command -v fly &> /dev/null; then
    echo "❌ Fly CLI is not installed. Please install it first:"
    echo "   curl -L https://fly.io/install.sh | sh"
    exit 1
fi

# Check if user is authenticated
if ! fly auth whoami &> /dev/null; then
    echo "❌ Not authenticated with Fly.io. Please run: fly auth login"
    exit 1
fi

# Check if app exists, if not create it
if ! fly apps list | grep -q "$APP_NAME"; then
    echo "📦 Creating new Fly.io app: $APP_NAME"
    fly apps create "$APP_NAME"
fi

# Build and deploy from root with proxy Dockerfile
echo "🔨 Building and deploying proxy..."
fly deploy --app "$APP_NAME" \
    --config apps/proxy/fly.toml \
    --dockerfile apps/proxy/Dockerfile

# Check deployment status
echo "✅ Checking deployment status..."
fly status --app "$APP_NAME"

# Run health check
echo "🏥 Running health check..."
APP_URL="https://${APP_NAME}.fly.dev"
if curl -f "${APP_URL}/proxy-health" > /dev/null 2>&1; then
    echo "✅ Health check passed!"
    echo "🌐 Proxy is running at: ${APP_URL}"
    echo "🔌 Proxy endpoint: ${APP_NAME}.fly.dev:8080"
else
    echo "⚠️  Health check failed. Please check the logs:"
    echo "   fly logs --app $APP_NAME"
fi