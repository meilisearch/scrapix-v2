#!/bin/bash

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}🚀 Deploying Scrapix Proxy to Multiple Regions${NC}"
echo ""

# Default regions (major Fly.io regions for global coverage)
DEFAULT_REGIONS=(
  "iad"  # Washington DC, USA (primary)
  "lax"  # Los Angeles, USA
  "ord"  # Chicago, USA
  "lhr"  # London, UK
  "fra"  # Frankfurt, Germany
  "ams"  # Amsterdam, Netherlands
  "sin"  # Singapore
  "syd"  # Sydney, Australia
  "nrt"  # Tokyo, Japan
  "gru"  # São Paulo, Brazil
)

# Parse command line arguments
REGIONS=()
SCALE_COUNT=1
APP_NAME="scrapix-proxy"

while [[ $# -gt 0 ]]; do
  case $1 in
    -r|--regions)
      IFS=',' read -r -a REGIONS <<< "$2"
      shift 2
      ;;
    -s|--scale)
      SCALE_COUNT="$2"
      shift 2
      ;;
    -a|--app)
      APP_NAME="$2"
      shift 2
      ;;
    --all)
      REGIONS=("${DEFAULT_REGIONS[@]}")
      shift
      ;;
    -h|--help)
      echo "Usage: $0 [options]"
      echo ""
      echo "Options:"
      echo "  -r, --regions <regions>  Comma-separated list of regions (e.g., iad,lhr,sin)"
      echo "  -s, --scale <count>      Number of machines per region (default: 1)"
      echo "  -a, --app <name>         App name (default: scrapix-proxy)"
      echo "  --all                    Deploy to all default regions"
      echo "  -h, --help              Show this help message"
      echo ""
      echo "Default regions: ${DEFAULT_REGIONS[*]}"
      exit 0
      ;;
    *)
      echo -e "${RED}Unknown option: $1${NC}"
      exit 1
      ;;
  esac
done

# Use default regions if none specified
if [ ${#REGIONS[@]} -eq 0 ]; then
  REGIONS=("${DEFAULT_REGIONS[@]}")
fi

# Navigate to proxy app directory
cd apps/proxy

# Check if app exists, create if it doesn't
echo -e "${YELLOW}Checking if app exists...${NC}"
if ! flyctl status --app "$APP_NAME" 2>/dev/null; then
  echo -e "${YELLOW}App doesn't exist. Creating...${NC}"
  flyctl apps create "$APP_NAME" --org meilisearch
fi

# Deploy to primary region first
PRIMARY_REGION="${REGIONS[0]}"
echo -e "${GREEN}Deploying to primary region: $PRIMARY_REGION${NC}"
flyctl deploy --app "$APP_NAME" --primary-region "$PRIMARY_REGION" --strategy immediate

# Wait for primary deployment
echo -e "${YELLOW}Waiting for primary deployment to stabilize...${NC}"
sleep 10

# Get current machines
echo -e "${YELLOW}Fetching current machines...${NC}"
EXISTING_MACHINES=$(flyctl machines list --app "$APP_NAME" --json | jq -r '.[].region' | sort -u)

# Deploy to additional regions
for REGION in "${REGIONS[@]:1}"; do
  echo ""
  echo -e "${GREEN}Deploying to region: $REGION${NC}"
  
  # Check if machines already exist in this region
  if echo "$EXISTING_MACHINES" | grep -q "^$REGION$"; then
    echo -e "${YELLOW}Machines already exist in $REGION, scaling...${NC}"
    flyctl scale count "$SCALE_COUNT" --region "$REGION" --app "$APP_NAME"
  else
    echo -e "${YELLOW}Cloning machine to $REGION...${NC}"
    # Get a machine ID from primary region
    MACHINE_ID=$(flyctl machines list --app "$APP_NAME" --json | jq -r ".[0].id")
    
    for i in $(seq 1 "$SCALE_COUNT"); do
      flyctl machines clone "$MACHINE_ID" --region "$REGION" --app "$APP_NAME"
    done
  fi
done

# Show deployment status
echo ""
echo -e "${GREEN}✅ Deployment complete!${NC}"
echo ""
echo -e "${YELLOW}Current deployment status:${NC}"
flyctl status --app "$APP_NAME"

echo ""
echo -e "${YELLOW}Machines by region:${NC}"
flyctl machines list --app "$APP_NAME" | grep -E "^[a-f0-9]{8}|Region"

echo ""
echo -e "${GREEN}Proxy endpoints:${NC}"
for REGION in "${REGIONS[@]}"; do
  echo "  - $REGION: $APP_NAME.$REGION.fly.dev:8080"
done

echo ""
echo -e "${GREEN}Global endpoint: $APP_NAME.fly.dev:8080${NC}"