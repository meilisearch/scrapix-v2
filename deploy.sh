#!/bin/bash

# Scrapix Fly.io Deployment Script
# This script handles the complete deployment process

set -e  # Exit on any error

echo "🚀 Starting Scrapix deployment to Fly.io..."

# Check if flyctl is installed
if ! command -v flyctl &> /dev/null; then
    echo "❌ flyctl is not installed. Please install it first:"
    echo "   curl -L https://fly.io/install.sh | sh"
    exit 1
fi

# Check if app exists
if ! flyctl apps show scrapix &> /dev/null; then
    echo "📦 Creating Fly app 'scrapix'..."
    flyctl apps create scrapix --org meilisearch
else
    echo "✅ App 'scrapix' already exists"
fi

# Set secrets if provided
echo "🔐 Setting up secrets..."
echo "   Please make sure to set the following secrets:"
echo "   - REDIS_URL (required for job queue)"
echo "   - OPENAI_API_KEY (optional, for AI features)"
echo ""
read -p "Do you want to set secrets now? (y/n) " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]]; then
    # Redis URL
    read -p "Enter REDIS_URL (or press Enter to skip): " REDIS_URL
    if [ ! -z "$REDIS_URL" ]; then
        flyctl secrets set REDIS_URL="$REDIS_URL" --app scrapix
    fi
    
    # OpenAI API Key
    read -p "Enter OPENAI_API_KEY (or press Enter to skip): " OPENAI_API_KEY
    if [ ! -z "$OPENAI_API_KEY" ]; then
        flyctl secrets set OPENAI_API_KEY="$OPENAI_API_KEY" --app scrapix
    fi
fi

# Deploy the application
echo "🚀 Deploying application..."
flyctl deploy --config fly.toml --wait-timeout 600

# Check deployment status
echo "📊 Checking deployment status..."
flyctl status --app scrapix

# Show app URL
echo ""
echo "✨ Deployment complete!"
echo "🌐 Your app is available at: https://scrapix.fly.dev"
echo ""
echo "📝 Useful commands:"
echo "   View logs:    flyctl logs --app scrapix"
echo "   SSH into app: flyctl ssh console --app scrapix"
echo "   View status:  flyctl status --app scrapix"
echo "   Scale app:    flyctl scale count 2 --app scrapix"