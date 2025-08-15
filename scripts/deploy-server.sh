#!/bin/bash

# Fly.io Deployment Script for Scrapix Server
# Usage: ./scripts/deploy-server.sh [staging|production]

set -e

ENVIRONMENT=${1:-production}
APP_NAME="scrapix"

if [ "$ENVIRONMENT" == "staging" ]; then
    APP_NAME="scrapix-staging"
fi

echo "🚀 Deploying Scrapix Server to Fly.io ($ENVIRONMENT)..."

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

# Build and deploy from root with server Dockerfile
echo "🔨 Building and deploying server..."
fly deploy --app "$APP_NAME" \
    --config apps/server/fly.toml \
    --dockerfile apps/server/Dockerfile

# Check deployment status
echo "✅ Checking deployment status..."
fly status --app "$APP_NAME"

# Run health check
echo "🏥 Running health check..."
APP_URL="https://${APP_NAME}.fly.dev"
if curl -f "${APP_URL}/health" > /dev/null 2>&1; then
    echo "✅ Health check passed!"
    echo "🌐 Server is running at: ${APP_URL}"
else
    echo "⚠️  Health check failed. Please check the logs:"
    echo "   fly logs --app $APP_NAME"
fi