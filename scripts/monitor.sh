#!/usr/bin/env bash
# Scrapix Real-time Monitoring Tool
# Monitors crawl progress with live metrics

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
MAGENTA='\033[0;35m'
NC='\033[0m'
BOLD='\033[1m'

# Default settings
LOG_DIR="./crawl-results/logs"
INTERVAL=2
COMPACT=false
CSV_OUTPUT=""

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS]

Scrapix Real-time Monitoring Tool - Live crawl metrics dashboard

OPTIONS:
    -d, --log-dir DIR    Log directory to monitor (default: ./crawl-results/logs)
    -i, --interval SECS  Refresh interval in seconds (default: 2)
    -c, --compact        Compact single-line output mode
    --csv FILE           Write metrics to CSV file
    -h, --help           Show this help

EXAMPLES:
    $(basename "$0")                           # Monitor with default settings
    $(basename "$0") -d /path/to/logs -i 1     # Custom log dir, 1s interval
    $(basename "$0") -c                        # Compact output mode
    $(basename "$0") --csv metrics.csv         # Also write to CSV

METRICS DISPLAYED:
    Crawler:  Pages processed, success rate, download speed
    Frontier: URLs discovered, duplicates filtered, dispatch rate
    Content:  Documents processed, indexing rate
    System:   Memory usage, active connections

EOF
}

parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -d|--log-dir)
                LOG_DIR="$2"
                shift 2
                ;;
            -i|--interval)
                INTERVAL="$2"
                shift 2
                ;;
            -c|--compact)
                COMPACT=true
                shift
                ;;
            --csv)
                CSV_OUTPUT="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                echo "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done
}

strip_ansi() {
    sed 's/\x1b\[[0-9;]*m//g'
}

get_crawler_metrics() {
    local log_file="$LOG_DIR/crawler.log"
    if [[ -f "$log_file" ]]; then
        grep -a "Worker metrics" "$log_file" 2>/dev/null | tail -1 | strip_ansi
    else
        echo ""
    fi
}

get_frontier_metrics() {
    local log_file="$LOG_DIR/frontier.log"
    if [[ -f "$log_file" ]]; then
        grep -a "Frontier metrics" "$log_file" 2>/dev/null | tail -1 | strip_ansi
    else
        echo ""
    fi
}

get_content_metrics() {
    local log_file="$LOG_DIR/content.log"
    if [[ -f "$log_file" ]]; then
        grep -a "Worker metrics" "$log_file" 2>/dev/null | tail -1 | strip_ansi
    else
        echo ""
    fi
}

parse_value() {
    local line="$1"
    local key="$2"
    echo "$line" | grep -oP "${key}=\\K[0-9]+" || echo "0"
}

format_number() {
    local num="$1"
    if [[ "$num" -ge 1000000 ]]; then
        printf "%.1fM" "$(echo "scale=1; $num / 1000000" | bc)"
    elif [[ "$num" -ge 1000 ]]; then
        printf "%.1fK" "$(echo "scale=1; $num / 1000" | bc)"
    else
        echo "$num"
    fi
}

format_bytes() {
    local mb="$1"
    if [[ "$mb" -ge 1024 ]]; then
        printf "%.1f GB" "$(echo "scale=1; $mb / 1024" | bc)"
    else
        echo "${mb} MB"
    fi
}

display_dashboard() {
    local crawler_line frontier_line content_line
    crawler_line=$(get_crawler_metrics)
    frontier_line=$(get_frontier_metrics)
    content_line=$(get_content_metrics)

    # Parse crawler metrics
    local c_processed c_succeeded c_failed c_bytes c_active c_dns_hits c_dns_misses
    c_processed=$(parse_value "$crawler_line" "processed")
    c_succeeded=$(parse_value "$crawler_line" "succeeded")
    c_failed=$(parse_value "$crawler_line" "failed")
    c_bytes=$(parse_value "$crawler_line" "bytes_mb")
    c_active=$(parse_value "$crawler_line" "active")
    c_dns_hits=$(parse_value "$crawler_line" "dns_hits")
    c_dns_misses=$(parse_value "$crawler_line" "dns_misses")

    # Parse frontier metrics
    local f_consumed f_new f_duplicate f_dispatched f_delayed
    f_consumed=$(parse_value "$frontier_line" "consumed")
    f_new=$(parse_value "$frontier_line" "new")
    f_duplicate=$(parse_value "$frontier_line" "duplicate")
    f_dispatched=$(parse_value "$frontier_line" "dispatched")
    f_delayed=$(parse_value "$frontier_line" "delayed")

    # Parse content metrics
    local ct_processed ct_succeeded ct_indexed ct_bytes
    ct_processed=$(parse_value "$content_line" "processed")
    ct_succeeded=$(parse_value "$content_line" "succeeded")
    ct_indexed=$(parse_value "$content_line" "docs_indexed")
    ct_bytes=$(parse_value "$content_line" "bytes_mb")

    # Calculate rates
    local success_rate dup_rate dns_rate
    if [[ "$c_processed" -gt 0 ]]; then
        success_rate=$(echo "scale=1; $c_succeeded * 100 / $c_processed" | bc)
    else
        success_rate="0"
    fi

    if [[ "$f_consumed" -gt 0 ]]; then
        dup_rate=$(echo "scale=1; $f_duplicate * 100 / $f_consumed" | bc)
    else
        dup_rate="0"
    fi

    local total_dns=$((c_dns_hits + c_dns_misses))
    if [[ "$total_dns" -gt 0 ]]; then
        dns_rate=$(echo "scale=1; $c_dns_hits * 100 / $total_dns" | bc)
    else
        dns_rate="0"
    fi

    if [[ "$COMPACT" == "true" ]]; then
        # Compact single-line output
        printf "\r${CYAN}[%s]${NC} Crawled: ${GREEN}%s${NC} | Failed: ${RED}%s${NC} | Active: ${YELLOW}%s${NC} | Downloaded: ${BLUE}%s${NC} | Pending: ${MAGENTA}%s${NC}     " \
            "$(date '+%H:%M:%S')" \
            "$(format_number "$c_succeeded")" \
            "$c_failed" \
            "$c_active" \
            "$(format_bytes "$c_bytes")" \
            "$(format_number "$f_delayed")"
    else
        # Full dashboard
        clear
        echo -e "${BOLD}╔════════════════════════════════════════════════════════════════╗${NC}"
        echo -e "${BOLD}║           SCRAPIX REAL-TIME MONITORING DASHBOARD               ║${NC}"
        echo -e "${BOLD}╠════════════════════════════════════════════════════════════════╣${NC}"
        echo -e "║  $(date '+%Y-%m-%d %H:%M:%S')                                          ║"
        echo -e "${BOLD}╠════════════════════════════════════════════════════════════════╣${NC}"

        # Crawler section
        echo -e "${BOLD}║ ${CYAN}CRAWLER${NC}                                                        ${BOLD}║${NC}"
        printf "║   Pages Processed: ${GREEN}%-10s${NC}  Succeeded: ${GREEN}%-10s${NC}        ║\n" \
            "$(format_number "$c_processed")" "$(format_number "$c_succeeded")"
        printf "║   Failed: ${RED}%-10s${NC}         Success Rate: ${GREEN}%s%%${NC}              ║\n" \
            "$c_failed" "$success_rate"
        printf "║   Downloaded: ${BLUE}%-12s${NC}   Active: ${YELLOW}%-5s${NC}                  ║\n" \
            "$(format_bytes "$c_bytes")" "$c_active"
        printf "║   DNS Cache Hit Rate: ${GREEN}%s%%${NC}                                  ║\n" \
            "$dns_rate"

        echo -e "${BOLD}╠════════════════════════════════════════════════════════════════╣${NC}"

        # Frontier section
        echo -e "${BOLD}║ ${MAGENTA}FRONTIER${NC}                                                       ${BOLD}║${NC}"
        printf "║   URLs Consumed: ${CYAN}%-12s${NC}  New: ${GREEN}%-12s${NC}        ║\n" \
            "$(format_number "$f_consumed")" "$(format_number "$f_new")"
        printf "║   Duplicates: ${YELLOW}%-12s${NC}    Duplicate Rate: ${YELLOW}%s%%${NC}        ║\n" \
            "$(format_number "$f_duplicate")" "$dup_rate"
        printf "║   Dispatched: ${GREEN}%-12s${NC}    Delayed: ${RED}%-12s${NC}        ║\n" \
            "$(format_number "$f_dispatched")" "$(format_number "$f_delayed")"

        echo -e "${BOLD}╠════════════════════════════════════════════════════════════════╣${NC}"

        # Content section
        echo -e "${BOLD}║ ${BLUE}CONTENT WORKER${NC}                                                  ${BOLD}║${NC}"
        printf "║   Processed: ${GREEN}%-12s${NC}     Indexed: ${GREEN}%-12s${NC}        ║\n" \
            "$(format_number "$ct_processed")" "$(format_number "$ct_indexed")"
        printf "║   Data Processed: ${BLUE}%-12s${NC}                                 ║\n" \
            "$(format_bytes "$ct_bytes")"

        echo -e "${BOLD}╠════════════════════════════════════════════════════════════════╣${NC}"

        # Progress bar
        local progress_pct=0
        if [[ "$f_new" -gt 0 ]]; then
            progress_pct=$((c_succeeded * 100 / f_new))
            [[ "$progress_pct" -gt 100 ]] && progress_pct=100
        fi
        local bar_width=50
        local filled=$((progress_pct * bar_width / 100))
        local empty=$((bar_width - filled))

        printf "║ Progress: [${GREEN}"
        printf '%*s' "$filled" '' | tr ' ' '█'
        printf "${NC}"
        printf '%*s' "$empty" '' | tr ' ' '░'
        printf "] %3d%%       ║\n" "$progress_pct"

        echo -e "${BOLD}╚════════════════════════════════════════════════════════════════╝${NC}"
        echo ""
        echo -e "Press ${BOLD}Ctrl+C${NC} to exit"
    fi

    # Write to CSV if requested
    if [[ -n "$CSV_OUTPUT" ]]; then
        if [[ ! -f "$CSV_OUTPUT" ]]; then
            echo "timestamp,crawler_processed,crawler_succeeded,crawler_failed,crawler_bytes_mb,crawler_active,frontier_consumed,frontier_new,frontier_duplicate,frontier_dispatched,frontier_delayed,content_processed,content_indexed" > "$CSV_OUTPUT"
        fi
        echo "$(date +%s),$c_processed,$c_succeeded,$c_failed,$c_bytes,$c_active,$f_consumed,$f_new,$f_duplicate,$f_dispatched,$f_delayed,$ct_processed,$ct_indexed" >> "$CSV_OUTPUT"
    fi
}

check_logs() {
    if [[ ! -d "$LOG_DIR" ]]; then
        echo -e "${RED}Log directory not found: $LOG_DIR${NC}"
        echo "Make sure a crawl is running or specify the correct log directory with -d"
        exit 1
    fi

    local has_logs=false
    for log in crawler.log frontier.log content.log; do
        if [[ -f "$LOG_DIR/$log" ]]; then
            has_logs=true
            break
        fi
    done

    if [[ "$has_logs" == "false" ]]; then
        echo -e "${YELLOW}No log files found in $LOG_DIR${NC}"
        echo "Waiting for logs to appear..."
    fi
}

main() {
    parse_args "$@"

    check_logs

    # Main monitoring loop
    while true; do
        display_dashboard
        sleep "$INTERVAL"
    done
}

main "$@"
