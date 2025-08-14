#!/bin/bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}🚀 Deploying Scrapix Proxy as Regional Apps${NC}"
echo ""

# Define regions
REGIONS="iad lax ord lhr fra ams sin syd nrt gru"

# Function to get app name for a region
get_app_name() {
  echo "scrapix-proxy-$1"
}

# Parse command line arguments
ACTION=${1:-deploy}
SPECIFIC_REGION=$2

# Navigate to proxy app directory
cd apps/proxy

# Function to deploy a single region
deploy_region() {
  local region=$1
  local app_name=$2
  
  echo -e "${GREEN}Deploying $app_name in region $region${NC}"
  
  # Check if app exists, create if it doesn't
  if ! flyctl status --app "$app_name" 2>/dev/null; then
    echo -e "${YELLOW}Creating app $app_name...${NC}"
    flyctl apps create "$app_name" --org meilisearch
  fi
  
  # Create a temporary fly.toml for this region
  cat > fly.toml.tmp <<EOF
# fly.toml app configuration file
app = "$app_name"
primary_region = "$region"

[build]

[env]
  PORT = "3000"
  PROXY_PORT = "8080"
  NODE_ENV = "production"
  REGION = "$region"

# HTTP management interface
[http_service]
  internal_port = 3000
  force_https = true
  auto_stop_machines = true
  auto_start_machines = true
  min_machines_running = 0
  processes = ["app"]
  
  [http_service.concurrency]
    type = "connections"
    hard_limit = 1000
    soft_limit = 800

[[http_service.checks]]
  grace_period = "10s"
  interval = "30s"
  method = "GET"
  timeout = "10s"
  path = "/proxy-health"

# TCP proxy service
[[services]]
  protocol = "tcp"
  internal_port = 8080
  auto_stop_machines = true
  auto_start_machines = true
  min_machines_running = 0
  
  [[services.ports]]
    port = 8080
    handlers = []
    
  [services.concurrency]
    type = "connections"
    hard_limit = 500
    soft_limit = 400
    
  [[services.tcp_checks]]
    interval = "15s"
    timeout = "5s"
    grace_period = "10s"

[vm]
  size = "shared-cpu-1x"
  memory = 1024

[[restart]]
  policy = "always"
  retries = 3
  window = "5m"

[metrics]
  port = 9091
  path = "/metrics"
EOF
  
  # Deploy the app
  echo -e "${YELLOW}Deploying to $region...${NC}"
  flyctl deploy --app "$app_name" --config fly.toml.tmp --primary-region "$region" --strategy immediate
  
  # Clean up temp file
  rm fly.toml.tmp
  
  echo -e "${GREEN}✅ $app_name deployed successfully!${NC}"
  echo "  - Health: https://${app_name}.fly.dev/proxy-health"
  echo "  - Proxy: ${app_name}.fly.dev:8080"
  echo ""
}

# Function to destroy a regional app
destroy_region() {
  local app_name=$1
  
  echo -e "${RED}Destroying $app_name...${NC}"
  flyctl apps destroy "$app_name" --yes 2>/dev/null || echo "App doesn't exist"
}

# Main execution
case $ACTION in
  deploy)
    if [ -n "$SPECIFIC_REGION" ]; then
      # Deploy specific region
      if echo "$REGIONS" | grep -q "$SPECIFIC_REGION"; then
        app_name=$(get_app_name "$SPECIFIC_REGION")
        deploy_region "$SPECIFIC_REGION" "$app_name"
      else
        echo -e "${RED}Unknown region: $SPECIFIC_REGION${NC}"
        exit 1
      fi
    else
      # Deploy all regions
      for region in $REGIONS; do
        app_name=$(get_app_name "$region")
        deploy_region "$region" "$app_name"
      done
      
      echo -e "${GREEN}🎉 All regional proxies deployed!${NC}"
      echo ""
      echo -e "${YELLOW}Available endpoints:${NC}"
      for region in $REGIONS; do
        app_name=$(get_app_name "$region")
        echo "  $region:"
        echo "    - Health: https://${app_name}.fly.dev/proxy-health"
        echo "    - Proxy: ${app_name}.fly.dev:8080"
      done
    fi
    ;;
    
  destroy)
    if [ -n "$SPECIFIC_REGION" ]; then
      # Destroy specific region
      if echo "$REGIONS" | grep -q "$SPECIFIC_REGION"; then
        app_name=$(get_app_name "$SPECIFIC_REGION")
        destroy_region "$app_name"
      else
        echo -e "${RED}Unknown region: $SPECIFIC_REGION${NC}"
        exit 1
      fi
    else
      # Destroy all regional apps
      echo -e "${RED}Destroying all regional proxy apps...${NC}"
      for region in $REGIONS; do
        app_name=$(get_app_name "$region")
        destroy_region "$app_name"
      done
    fi
    ;;
    
  status)
    echo -e "${YELLOW}Regional Proxy Apps Status:${NC}"
    for region in $REGIONS; do
      app_name=$(get_app_name "$region")
      echo -n "  $app_name ($region): "
      if flyctl status --app "$app_name" 2>/dev/null | grep -q "Deployed"; then
        echo -e "${GREEN}✓ Deployed${NC}"
      else
        echo -e "${RED}✗ Not deployed${NC}"
      fi
    done
    ;;
    
  *)
    echo "Usage: $0 [deploy|destroy|status] [region]"
    echo ""
    echo "Commands:"
    echo "  deploy        Deploy proxy apps (all or specific region)"
    echo "  destroy       Destroy proxy apps (all or specific region)"
    echo "  status        Check status of all regional apps"
    echo ""
    echo "Regions: iad, lax, ord, lhr, fra, ams, sin, syd, nrt, gru"
    echo ""
    echo "Examples:"
    echo "  $0 deploy          # Deploy all regions"
    echo "  $0 deploy lhr      # Deploy only London"
    echo "  $0 destroy sin     # Destroy Singapore app"
    echo "  $0 status          # Check all apps"
    exit 0
    ;;
esac