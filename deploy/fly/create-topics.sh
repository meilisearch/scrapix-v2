#!/usr/bin/env bash
# Create the 7 Kafka topics Scrapix expects, with the partition counts used
# in docker-compose.yml. Idempotent: `rpk topic create` skips existing topics.
#
# Usage: ./deploy/fly/create-topics.sh

set -euo pipefail

APP="${APP:-scrapix-redpanda}"
BROKERS="${BROKERS:-scrapix-redpanda.internal:9092}"

run_rpk() {
    flyctl ssh console --app "$APP" --command "rpk $* --brokers $BROKERS"
}

echo "Creating Kafka topics on $APP..."
run_rpk "topic create scrapix.urls.frontier    -p 12 -c retention.ms=604800000"
run_rpk "topic create scrapix.urls.processing  -p 12 -c retention.ms=604800000"
run_rpk "topic create scrapix.pages.raw        -p 6"
run_rpk "topic create scrapix.documents        -p 6"
run_rpk "topic create scrapix.events           -p 3"
run_rpk "topic create scrapix.dlq.urls         -p 3"
run_rpk "topic create scrapix.jobs.status      -p 3"

echo "Done. Listing topics:"
run_rpk "topic list"
