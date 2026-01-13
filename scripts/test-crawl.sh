#!/usr/bin/env bash
# Scrapix Test Crawl Tool
# Runs crawl tests with timing and metrics collection

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Process tracking
API_PID=""
FRONTIER_PID=""
CRAWLER_PID=""
CONTENT_PID=""
MONITOR_PID=""

# Default settings
CONFIG_FILE=""
MAX_PAGES=1000
CONCURRENCY=50
DOMAIN_DELAY_MS=50
DISPATCH_BATCH_SIZE=500
LOG_LEVEL="info"
OUTPUT_DIR="./crawl-results"
WAIT_FOR_COMPLETION=true
MONITOR_INTERVAL=5

# Timestamps
START_TIME=""
END_TIME=""

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS] <CONFIG_FILE|PRESET>

Scrapix Test Crawl Tool - Run crawls with comprehensive timing and metrics

PRESETS:
    wikipedia-1k        Crawl 1,000 Wikipedia pages
    wikipedia-10k       Crawl 10,000 Wikipedia pages
    wikipedia-100k      Crawl 100,000 Wikipedia pages
    meilisearch-docs    Crawl Meilisearch documentation

OPTIONS:
    -c, --concurrency N     Max concurrent requests (default: 50)
    -m, --max-pages N       Maximum pages to crawl (default: 1000)
    -d, --domain-delay MS   Delay between requests to same domain (default: 50)
    -b, --batch-size N      Frontier dispatch batch size (default: 500)
    -l, --log-level LEVEL   Log level: debug, info, warn, error (default: info)
    -o, --output DIR        Output directory for results (default: ./crawl-results)
    -n, --no-wait           Don't wait for completion, just start services
    -i, --interval SECS     Metrics monitoring interval (default: 5)
    -h, --help              Show this help

EXAMPLES:
    $(basename "$0") wikipedia-10k
    $(basename "$0") -c 100 -m 5000 examples/simple-crawl.json
    $(basename "$0") -d 20 -b 1000 wikipedia-1k
    $(basename "$0") --no-wait wikipedia-10k

EOF
}

log_info() { echo -e "${BLUE}[INFO]${NC} $(date '+%H:%M:%S') $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $(date '+%H:%M:%S') $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $(date '+%H:%M:%S') $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $(date '+%H:%M:%S') $*" >&2; }
log_metric() { echo -e "${CYAN}[METRIC]${NC} $*"; }

cleanup() {
    log_info "Cleaning up..."
    [[ -n "$MONITOR_PID" ]] && kill "$MONITOR_PID" 2>/dev/null || true
    [[ -n "$API_PID" ]] && kill "$API_PID" 2>/dev/null || true
    [[ -n "$FRONTIER_PID" ]] && kill "$FRONTIER_PID" 2>/dev/null || true
    [[ -n "$CRAWLER_PID" ]] && kill "$CRAWLER_PID" 2>/dev/null || true
    [[ -n "$CONTENT_PID" ]] && kill "$CONTENT_PID" 2>/dev/null || true
    wait 2>/dev/null || true
    log_info "All services stopped"
}

trap cleanup EXIT

parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -c|--concurrency)
                CONCURRENCY="$2"
                shift 2
                ;;
            -m|--max-pages)
                MAX_PAGES="$2"
                shift 2
                ;;
            -d|--domain-delay)
                DOMAIN_DELAY_MS="$2"
                shift 2
                ;;
            -b|--batch-size)
                DISPATCH_BATCH_SIZE="$2"
                shift 2
                ;;
            -l|--log-level)
                LOG_LEVEL="$2"
                shift 2
                ;;
            -o|--output)
                OUTPUT_DIR="$2"
                shift 2
                ;;
            -n|--no-wait)
                WAIT_FOR_COMPLETION=false
                shift
                ;;
            -i|--interval)
                MONITOR_INTERVAL="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            wikipedia-1k|wikipedia-10k|wikipedia-100k|meilisearch-docs)
                setup_preset "$1"
                shift
                ;;
            *.json)
                CONFIG_FILE="$1"
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done

    if [[ -z "$CONFIG_FILE" ]]; then
        log_error "No config file or preset specified"
        usage
        exit 1
    fi
}

setup_preset() {
    local preset="$1"
    case "$preset" in
        wikipedia-1k)
            MAX_PAGES=1000
            CONCURRENCY=50
            DOMAIN_DELAY_MS=50
            create_wikipedia_config 1000
            ;;
        wikipedia-10k)
            MAX_PAGES=10000
            CONCURRENCY=100
            DOMAIN_DELAY_MS=20
            DISPATCH_BATCH_SIZE=1000
            create_wikipedia_config 10000
            ;;
        wikipedia-100k)
            MAX_PAGES=100000
            CONCURRENCY=200
            DOMAIN_DELAY_MS=10
            DISPATCH_BATCH_SIZE=2000
            create_wikipedia_config 100000
            ;;
        meilisearch-docs)
            MAX_PAGES=1000
            CONCURRENCY=20
            DOMAIN_DELAY_MS=100
            CONFIG_FILE="examples/simple-crawl.json"
            ;;
    esac
}

create_wikipedia_config() {
    local max_pages="$1"
    local config_dir="/tmp/scrapix-test"
    mkdir -p "$config_dir"
    CONFIG_FILE="$config_dir/wikipedia-${max_pages}.json"

    cat > "$CONFIG_FILE" << EOF
{
  "start_urls": [
    "https://en.wikipedia.org/wiki/Main_Page",
    "https://en.wikipedia.org/wiki/Portal:Featured_content",
    "https://en.wikipedia.org/wiki/Portal:Current_events"
  ],
  "index_uid": "wikipedia-test-${max_pages}",
  "crawler_type": "http",
  "max_depth": 4,
  "max_pages": ${max_pages},
  "url_patterns": {
    "include": ["https://en.wikipedia.org/wiki/**"],
    "exclude": [
      "**/Special:**", "**/Talk:**", "**/Wikipedia:**",
      "**/Help:**", "**/Category:**", "**/Portal:**",
      "**/File:**", "**/Template:**", "**/User:**",
      "**?oldid=**", "**?action=**", "**#**"
    ]
  },
  "concurrency": {
    "max_concurrent_requests": ${CONCURRENCY}
  },
  "rate_limit": {
    "requests_per_second": 100,
    "per_domain_delay_ms": ${DOMAIN_DELAY_MS},
    "respect_robots_txt": true
  },
  "features": {
    "metadata": { "enabled": true },
    "markdown": { "enabled": true }
  },
  "meilisearch": {
    "url": "http://localhost:7700",
    "api_key": "masterKey",
    "batch_size": 500
  }
}
EOF
    log_info "Created config: $CONFIG_FILE"
}

ensure_infrastructure() {
    log_info "Checking infrastructure..."

    # Check Redpanda
    if ! curl -s http://localhost:19092 > /dev/null 2>&1; then
        log_info "Starting infrastructure with docker-compose..."
        docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d redpanda meilisearch dragonfly
        sleep 5
    fi

    # Check Meilisearch
    local retries=30
    while [[ $retries -gt 0 ]]; do
        if curl -s http://localhost:7700/health | grep -q "available"; then
            log_success "Meilisearch is healthy"
            break
        fi
        sleep 1
        retries=$((retries - 1))
    done

    if [[ $retries -eq 0 ]]; then
        log_error "Meilisearch failed to start"
        exit 1
    fi
}

ensure_build() {
    log_info "Building release binaries..."
    cargo build --release --bin scrapix-api --bin scrapix-worker-crawler \
        --bin scrapix-worker-content --bin scrapix-frontier-service --bin scrapix 2>&1 | tail -3
    log_success "Build complete"
}

start_services() {
    local log_dir="$OUTPUT_DIR/logs"
    mkdir -p "$log_dir"

    log_info "Starting services..."

    # API Server
    RUST_LOG="$LOG_LEVEL,scrapix=$LOG_LEVEL" \
    KAFKA_BROKERS=localhost:19092 \
    MEILISEARCH_URL=http://localhost:7700 \
    MEILISEARCH_API_KEY=masterKey \
    ./target/release/scrapix-api > "$log_dir/api.log" 2>&1 &
    API_PID=$!

    # Frontier Service
    RUST_LOG="$LOG_LEVEL,scrapix=$LOG_LEVEL" \
    KAFKA_BROKERS=localhost:19092 \
    DISPATCH_INTERVAL_MS=50 \
    DISPATCH_BATCH_SIZE="$DISPATCH_BATCH_SIZE" \
    DOMAIN_DELAY_MS="$DOMAIN_DELAY_MS" \
    BLOOM_CAPACITY=1000000 \
    ./target/release/scrapix-frontier-service > "$log_dir/frontier.log" 2>&1 &
    FRONTIER_PID=$!

    # Crawler Worker
    RUST_LOG="$LOG_LEVEL,scrapix=$LOG_LEVEL" \
    KAFKA_BROKERS=localhost:19092 \
    CONCURRENCY="$CONCURRENCY" \
    MAX_DEPTH=10 \
    RESPECT_ROBOTS=true \
    ./target/release/scrapix-worker-crawler > "$log_dir/crawler.log" 2>&1 &
    CRAWLER_PID=$!

    # Content Worker
    RUST_LOG="$LOG_LEVEL,scrapix=$LOG_LEVEL" \
    KAFKA_BROKERS=localhost:19092 \
    MEILISEARCH_URL=http://localhost:7700 \
    MEILISEARCH_API_KEY=masterKey \
    BATCH_SIZE=200 \
    BATCH_TIMEOUT_SECS=2 \
    ./target/release/scrapix-worker-content > "$log_dir/content.log" 2>&1 &
    CONTENT_PID=$!

    sleep 3
    log_success "All services started (PIDs: API=$API_PID, Frontier=$FRONTIER_PID, Crawler=$CRAWLER_PID, Content=$CONTENT_PID)"
}

start_metrics_monitor() {
    local metrics_file="$OUTPUT_DIR/metrics.csv"

    echo "timestamp,elapsed_secs,crawler_processed,crawler_succeeded,crawler_failed,crawler_bytes_mb,crawler_active,frontier_new,frontier_duplicate,frontier_dispatched,frontier_delayed,content_processed,content_indexed" > "$metrics_file"

    (
        while true; do
            sleep "$MONITOR_INTERVAL"

            local elapsed=$(($(date +%s) - START_TIME))

            # Parse crawler metrics
            local crawler_line
            crawler_line=$(grep -a "Worker metrics" "$OUTPUT_DIR/logs/crawler.log" 2>/dev/null | tail -1 | sed 's/\x1b\[[0-9;]*m//g' || echo "")
            local crawler_processed=$(echo "$crawler_line" | grep -oP 'processed=\K\d+' || echo "0")
            local crawler_succeeded=$(echo "$crawler_line" | grep -oP 'succeeded=\K\d+' || echo "0")
            local crawler_failed=$(echo "$crawler_line" | grep -oP 'failed=\K\d+' || echo "0")
            local crawler_bytes=$(echo "$crawler_line" | grep -oP 'bytes_mb=\K\d+' || echo "0")
            local crawler_active=$(echo "$crawler_line" | grep -oP 'active=\K\d+' || echo "0")

            # Parse frontier metrics
            local frontier_line
            frontier_line=$(grep -a "Frontier metrics" "$OUTPUT_DIR/logs/frontier.log" 2>/dev/null | tail -1 | sed 's/\x1b\[[0-9;]*m//g' || echo "")
            local frontier_new=$(echo "$frontier_line" | grep -oP 'new=\K\d+' || echo "0")
            local frontier_duplicate=$(echo "$frontier_line" | grep -oP 'duplicate=\K\d+' || echo "0")
            local frontier_dispatched=$(echo "$frontier_line" | grep -oP 'dispatched=\K\d+' || echo "0")
            local frontier_delayed=$(echo "$frontier_line" | grep -oP 'delayed=\K\d+' || echo "0")

            # Parse content metrics
            local content_line
            content_line=$(grep -a "Worker metrics" "$OUTPUT_DIR/logs/content.log" 2>/dev/null | tail -1 | sed 's/\x1b\[[0-9;]*m//g' || echo "")
            local content_processed=$(echo "$content_line" | grep -oP 'processed=\K\d+' || echo "0")
            local content_indexed=$(echo "$content_line" | grep -oP 'docs_indexed=\K\d+' || echo "0")

            # Write to CSV
            echo "$(date +%s),$elapsed,$crawler_processed,$crawler_succeeded,$crawler_failed,$crawler_bytes,$crawler_active,$frontier_new,$frontier_duplicate,$frontier_dispatched,$frontier_delayed,$content_processed,$content_indexed" >> "$metrics_file"

            # Calculate rates
            local rate="0"
            if [[ "$elapsed" -gt 0 && "$crawler_succeeded" -gt 0 ]]; then
                rate=$(echo "scale=1; $crawler_succeeded / $elapsed" | bc)
            fi

            # Display progress
            printf "\r${CYAN}[%ds]${NC} Crawled: %d/%d (%.1f/s) | Indexed: %d | Pending: %d | Active: %d     " \
                "$elapsed" "$crawler_succeeded" "$MAX_PAGES" "$rate" "$content_indexed" "$frontier_delayed" "$crawler_active"

            # Check completion
            if [[ "$crawler_succeeded" -ge "$MAX_PAGES" ]]; then
                echo ""
                log_success "Target reached: $crawler_succeeded pages crawled"
                break
            fi
        done
    ) &
    MONITOR_PID=$!
}

start_crawl() {
    log_info "Starting crawl with config: $CONFIG_FILE"

    local job_response
    job_response=$(./target/release/scrapix crawl -p "$CONFIG_FILE" 2>&1)

    if echo "$job_response" | grep -q "Job created"; then
        local job_id
        job_id=$(echo "$job_response" | grep -oP 'Job created: \K[a-f0-9-]+' || echo "unknown")
        log_success "Crawl job started: $job_id"
        echo "$job_id" > "$OUTPUT_DIR/job_id.txt"
    else
        log_error "Failed to start crawl: $job_response"
        exit 1
    fi
}

generate_report() {
    local report_file="$OUTPUT_DIR/report.txt"
    local metrics_file="$OUTPUT_DIR/metrics.csv"

    END_TIME=$(date +%s)
    local duration=$((END_TIME - START_TIME))

    # Get final metrics
    local final_line
    final_line=$(tail -1 "$metrics_file")
    local final_crawled=$(echo "$final_line" | cut -d',' -f4)
    local final_indexed=$(echo "$final_line" | cut -d',' -f13)
    local final_bytes=$(echo "$final_line" | cut -d',' -f6)

    local rate="0"
    if [[ "$duration" -gt 0 ]]; then
        rate=$(echo "scale=2; $final_crawled / $duration" | bc)
    fi

    cat > "$report_file" << EOF
========================================
  SCRAPIX CRAWL REPORT
========================================

Configuration:
  Config File:     $CONFIG_FILE
  Max Pages:       $MAX_PAGES
  Concurrency:     $CONCURRENCY
  Domain Delay:    ${DOMAIN_DELAY_MS}ms
  Batch Size:      $DISPATCH_BATCH_SIZE

Results:
  Duration:        ${duration}s
  Pages Crawled:   $final_crawled
  Pages Indexed:   $final_indexed
  Data Downloaded: ${final_bytes}MB
  Average Rate:    ${rate} pages/sec

Timing Breakdown:
  Start Time:      $(date -r $START_TIME '+%Y-%m-%d %H:%M:%S')
  End Time:        $(date -r $END_TIME '+%Y-%m-%d %H:%M:%S')

Log Files:
  API:             $OUTPUT_DIR/logs/api.log
  Frontier:        $OUTPUT_DIR/logs/frontier.log
  Crawler:         $OUTPUT_DIR/logs/crawler.log
  Content:         $OUTPUT_DIR/logs/content.log
  Metrics CSV:     $OUTPUT_DIR/metrics.csv

========================================
EOF

    cat "$report_file"
    log_success "Report saved to: $report_file"
}

main() {
    parse_args "$@"

    mkdir -p "$OUTPUT_DIR"

    echo ""
    echo "=========================================="
    echo "   Scrapix Test Crawl Tool"
    echo "=========================================="
    echo ""
    log_info "Config:      $CONFIG_FILE"
    log_info "Max Pages:   $MAX_PAGES"
    log_info "Concurrency: $CONCURRENCY"
    log_info "Domain Delay: ${DOMAIN_DELAY_MS}ms"
    log_info "Output:      $OUTPUT_DIR"
    echo ""

    ensure_infrastructure
    ensure_build
    start_services

    START_TIME=$(date +%s)

    if [[ "$WAIT_FOR_COMPLETION" == "true" ]]; then
        start_metrics_monitor
    fi

    start_crawl

    if [[ "$WAIT_FOR_COMPLETION" == "true" ]]; then
        log_info "Waiting for crawl to complete (target: $MAX_PAGES pages)..."
        wait "$MONITOR_PID" 2>/dev/null || true
        generate_report
    else
        log_info "Services running. Use 'kill $API_PID $FRONTIER_PID $CRAWLER_PID $CONTENT_PID' to stop."
        log_info "Monitor logs in: $OUTPUT_DIR/logs/"
        # Keep running
        wait
    fi
}

main "$@"
