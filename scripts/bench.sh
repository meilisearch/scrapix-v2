#!/usr/bin/env bash
# Scrapix Benchmarking Tool
# Runs benchmarks and collects timing metrics

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
cd "$PROJECT_ROOT"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default settings
BENCH_TYPE="all"
OUTPUT_DIR="./bench-results"
ITERATIONS=1
VERBOSE=false

usage() {
    cat << EOF
Usage: $(basename "$0") [OPTIONS] [BENCHMARK]

Scrapix Benchmarking Tool - Run benchmarks with timing metrics

BENCHMARKS:
    all                 Run all benchmarks (default)
    wikipedia           Wikipedia end-to-end benchmark
    integrated          Integrated component benchmarks
    parser              Parser benchmarks only
    frontier            Frontier benchmarks only

OPTIONS:
    -o, --output DIR    Output directory for results (default: ./bench-results)
    -i, --iterations N  Number of iterations (default: 1)
    -v, --verbose       Verbose output
    -h, --help          Show this help

EXAMPLES:
    $(basename "$0")                    # Run all benchmarks
    $(basename "$0") wikipedia          # Run Wikipedia E2E benchmark
    $(basename "$0") -i 3 integrated    # Run integrated benchmarks 3 times
    $(basename "$0") -o results parser  # Save parser bench results to ./results

EOF
}

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_success() { echo -e "${GREEN}[OK]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_error() { echo -e "${RED}[ERROR]${NC} $*" >&2; }

parse_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -o|--output)
                OUTPUT_DIR="$2"
                shift 2
                ;;
            -i|--iterations)
                ITERATIONS="$2"
                shift 2
                ;;
            -v|--verbose)
                VERBOSE=true
                shift
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            all|wikipedia|integrated|parser|frontier)
                BENCH_TYPE="$1"
                shift
                ;;
            *)
                log_error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done
}

ensure_build() {
    log_info "Ensuring release build is up to date..."
    if [[ "$VERBOSE" == "true" ]]; then
        cargo build --release 2>&1
    else
        cargo build --release 2>&1 | tail -5
    fi
}

run_bench() {
    local bench_name="$1"
    local start_time end_time duration

    log_info "Running benchmark: $bench_name"

    mkdir -p "$OUTPUT_DIR"
    local output_file="$OUTPUT_DIR/${bench_name}-$(date +%Y%m%d-%H%M%S).txt"

    start_time=$(date +%s%3N)

    if [[ "$VERBOSE" == "true" ]]; then
        cargo bench --bench "$bench_name" 2>&1 | tee "$output_file"
    else
        cargo bench --bench "$bench_name" > "$output_file" 2>&1
    fi

    end_time=$(date +%s%3N)
    duration=$((end_time - start_time))

    log_success "Benchmark '$bench_name' completed in ${duration}ms"
    log_info "Results saved to: $output_file"

    # Extract and display key metrics
    echo ""
    echo "=== Key Results ==="
    grep -E "(time:|thrpt:|Benchmarking)" "$output_file" | head -20 || true
    echo ""
}

run_all_benches() {
    local total_start total_end total_duration

    total_start=$(date +%s%3N)

    for i in $(seq 1 "$ITERATIONS"); do
        if [[ "$ITERATIONS" -gt 1 ]]; then
            log_info "=== Iteration $i of $ITERATIONS ==="
        fi

        case "$BENCH_TYPE" in
            all)
                run_bench "wikipedia_e2e"
                run_bench "integrated_benchmarks"
                ;;
            wikipedia)
                run_bench "wikipedia_e2e"
                ;;
            integrated)
                run_bench "integrated_benchmarks"
                ;;
            parser)
                run_bench "integrated_benchmarks" # contains parser benches
                ;;
            frontier)
                run_bench "integrated_benchmarks" # contains frontier benches
                ;;
        esac
    done

    total_end=$(date +%s%3N)
    total_duration=$((total_end - total_start))

    echo ""
    log_success "=== All benchmarks completed in ${total_duration}ms ==="
}

main() {
    parse_args "$@"

    echo "=========================================="
    echo "   Scrapix Benchmarking Tool"
    echo "=========================================="
    echo ""
    log_info "Benchmark type: $BENCH_TYPE"
    log_info "Output directory: $OUTPUT_DIR"
    log_info "Iterations: $ITERATIONS"
    echo ""

    ensure_build
    run_all_benches
}

main "$@"
