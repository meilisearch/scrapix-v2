#!/usr/bin/env bash
# =============================================================================
# Scrapix Heroku Setup Script
# =============================================================================
# Provisions two Heroku apps:
#   1. scrapix-api    — Rust API (scrapix all mode) + Postgres addon
#   2. scrapix-console — Next.js frontend
#
# Prerequisites:
#   - Heroku CLI installed and logged in (`heroku login`)
#   - A Meilisearch Cloud instance (or self-hosted)
#   - (Optional) ClickHouse Cloud instance
#   - (Optional) Anthropic API key for AI enrichment
#
# Usage:
#   1. Edit the variables below
#   2. Run: bash deploy/heroku/setup.sh
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration — edit these before running
# ---------------------------------------------------------------------------
API_APP_NAME="scrapix-api"
CONSOLE_APP_NAME="scrapix-console"

# Meilisearch Cloud
MEILISEARCH_URL=""        # e.g. https://ms-xxxx.meilisearch.io
MEILISEARCH_API_KEY=""    # master key

# ClickHouse (optional — leave empty to disable analytics)
CLICKHOUSE_URL=""         # e.g. https://ka2htxje0a.eu-central-1.aws.clickhouse.cloud:8443
CLICKHOUSE_DATABASE="scrapix_prod"
CLICKHOUSE_USER="default"
CLICKHOUSE_PASSWORD=""

# AI enrichment (optional)
AI_PROVIDER="anthropic"
ANTHROPIC_API_KEY=""

# CORS — the console URL will be added automatically
# Add extra origins here (comma-separated), *.meilisearch.com is always allowed
CORS_ORIGINS=""

# ---------------------------------------------------------------------------
# Create API app
# ---------------------------------------------------------------------------
echo "==> Creating API app: ${API_APP_NAME}"
heroku create "${API_APP_NAME}" --stack container

echo "==> Adding Postgres addon"
heroku addons:create heroku-postgresql:essential-0 -a "${API_APP_NAME}"

echo "==> Setting API config vars"
JWT_SECRET=$(openssl rand -hex 32)

# Build config vars, skipping empty optional ones
CONFIG_VARS=(
    "JWT_SECRET=${JWT_SECRET}"
    "RUST_LOG=info"
)

[ -n "${MEILISEARCH_URL}" ]     && CONFIG_VARS+=("MEILISEARCH_URL=${MEILISEARCH_URL}")
[ -n "${MEILISEARCH_API_KEY}" ] && CONFIG_VARS+=("MEILISEARCH_API_KEY=${MEILISEARCH_API_KEY}")
[ -n "${CLICKHOUSE_URL}" ]      && CONFIG_VARS+=("CLICKHOUSE_URL=${CLICKHOUSE_URL}")
[ -n "${CLICKHOUSE_DATABASE}" ] && CONFIG_VARS+=("CLICKHOUSE_DATABASE=${CLICKHOUSE_DATABASE}")
[ -n "${CLICKHOUSE_USER}" ]     && CONFIG_VARS+=("CLICKHOUSE_USER=${CLICKHOUSE_USER}")
[ -n "${CLICKHOUSE_PASSWORD}" ] && CONFIG_VARS+=("CLICKHOUSE_PASSWORD=${CLICKHOUSE_PASSWORD}")
[ -n "${AI_PROVIDER}" ]         && CONFIG_VARS+=("AI_PROVIDER=${AI_PROVIDER}")
[ -n "${ANTHROPIC_API_KEY}" ]   && CONFIG_VARS+=("ANTHROPIC_API_KEY=${ANTHROPIC_API_KEY}")

# Add console URL to CORS origins
CONSOLE_URL="https://${CONSOLE_APP_NAME}.herokuapp.com"
if [ -n "${CORS_ORIGINS}" ]; then
    CONFIG_VARS+=("CORS_ORIGINS=${CORS_ORIGINS},${CONSOLE_URL}")
else
    CONFIG_VARS+=("CORS_ORIGINS=${CONSOLE_URL}")
fi

heroku config:set -a "${API_APP_NAME}" "${CONFIG_VARS[@]}"

# ---------------------------------------------------------------------------
# Create Console app
# ---------------------------------------------------------------------------
echo ""
echo "==> Creating Console app: ${CONSOLE_APP_NAME}"
heroku create "${CONSOLE_APP_NAME}" --stack container

API_URL="https://${API_APP_NAME}.herokuapp.com"

echo "==> Setting Console config vars"
heroku config:set -a "${CONSOLE_APP_NAME}" \
    SCRAPIX_API_URL="${API_URL}" \
    NEXT_PUBLIC_SCRAPIX_API_URL="${API_URL}"

# ---------------------------------------------------------------------------
# Deploy instructions
# ---------------------------------------------------------------------------
echo ""
echo "============================================"
echo " Setup complete!"
echo "============================================"
echo ""
echo "Next steps:"
echo ""
echo "  1. Deploy the API (from repo root):"
echo "     git remote add heroku-api https://git.heroku.com/${API_APP_NAME}.git"
echo "     git push heroku-api main"
echo ""
echo "  2. Deploy the Console (from console/ dir):"
echo "     cd console"
echo "     git remote add heroku-console https://git.heroku.com/${CONSOLE_APP_NAME}.git"
echo "     git subtree push --prefix console heroku-console main"
echo "     # Or use a separate repo for the console"
echo ""
echo "  3. Check logs:"
echo "     heroku logs -a ${API_APP_NAME} --tail"
echo "     heroku logs -a ${CONSOLE_APP_NAME} --tail"
echo ""
echo "  API URL:     ${API_URL}"
echo "  Console URL: ${CONSOLE_URL}"
echo ""
