# Scrapix Development Justfile
# Usage: just dev       — start everything (infra + services + console)
#        just infra     — start infrastructure only
#        just services  — start Rust services + console (assumes infra is up)
#        just stop      — stop everything
#        just logs NAME — attach to a specific overmind process (api, frontier, crawler, content, console)

set dotenv-load

# Default: show available commands
default:
    @just --list

# ---------------------------------------------------------------------------
# Full stack
# ---------------------------------------------------------------------------

# Start infrastructure, wait for health, then run all services
dev: infra _wait-healthy
    overmind start -f Procfile.dev -N

# ---------------------------------------------------------------------------
# Infrastructure
# ---------------------------------------------------------------------------

# Start infrastructure services (Redpanda, Meilisearch, DragonflyDB, PostgreSQL, ClickHouse)
infra:
    docker compose -f docker-compose.yml -f docker-compose.dev.yml up -d

# Stop infrastructure
infra-down:
    docker compose -f docker-compose.yml -f docker-compose.dev.yml down

# Reset infrastructure (removes all data volumes)
infra-reset:
    docker compose -f docker-compose.yml -f docker-compose.dev.yml down -v

# Show infrastructure logs
infra-logs *ARGS:
    docker compose -f docker-compose.yml -f docker-compose.dev.yml logs {{ ARGS }}

# ---------------------------------------------------------------------------
# Services (assumes infra is running)
# ---------------------------------------------------------------------------

# Start all Rust services + console via overmind
services:
    overmind start -f Procfile.dev -N

# Attach to a specific service's log output
logs name:
    overmind connect {{ name }}

# Restart a single service
restart name:
    overmind restart {{ name }}

# Stop all overmind-managed services
stop-services:
    -overmind quit

# ---------------------------------------------------------------------------
# Individual services (for when you only need one)
# ---------------------------------------------------------------------------

# Run API server with cargo-watch
api:
    cargo watch -w crates -w bins -x 'run --bin scrapix -- api'

# Run frontier service with cargo-watch
frontier:
    cargo watch -w crates -w bins -x 'run --bin scrapix -- frontier'

# Run crawler worker with cargo-watch
crawler:
    cargo watch -w crates -w bins -x 'run --bin scrapix -- crawler'

# Run content worker with cargo-watch
content:
    cargo watch -w crates -w bins -x 'run --bin scrapix -- content'

# Run Next.js console
console:
    cd console && npm run dev

# ---------------------------------------------------------------------------
# Build & Test
# ---------------------------------------------------------------------------

# Type-check all crates without building
check:
    cargo check --workspace

# Build all crates in debug mode
build:
    cargo build --workspace

# Build all crates in release mode
build-release:
    cargo build --workspace --release

# Run all tests
test:
    cargo test --workspace

# Run tests for a specific crate
test-crate crate:
    cargo test -p {{ crate }}

# Run clippy lints
lint:
    cargo clippy --workspace -- -D warnings

# Format all code
fmt:
    cargo fmt --all

# ---------------------------------------------------------------------------
# Stop everything
# ---------------------------------------------------------------------------

# Stop services and infrastructure
stop: stop-services infra-down

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

# Wait for all infrastructure services to be healthy
[private]
_wait-healthy:
    #!/usr/bin/env bash
    set -euo pipefail
    echo "Waiting for infrastructure..."
    services=("scrapix-redpanda" "scrapix-dragonfly" "scrapix-meilisearch" "scrapix-postgres" "scrapix-clickhouse")
    for svc in "${services[@]}"; do
        printf "  %-25s " "$svc"
        timeout=60
        while [ $timeout -gt 0 ]; do
            status=$(docker inspect --format='{{"{{"}}.State.Health.Status{{"}}"}}' "$svc" 2>/dev/null || echo "missing")
            if [ "$status" = "healthy" ]; then
                echo "ready"
                break
            fi
            sleep 1
            timeout=$((timeout - 1))
        done
        if [ $timeout -eq 0 ]; then
            echo "TIMEOUT (still $status)"
            exit 1
        fi
    done
    # Wait for topic init to complete
    printf "  %-25s " "scrapix-init-topics"
    timeout=30
    while [ $timeout -gt 0 ]; do
        status=$(docker inspect --format='{{"{{"}}.State.Status{{"}}"}}' "scrapix-init-topics" 2>/dev/null || echo "missing")
        if [ "$status" = "exited" ]; then
            exit_code=$(docker inspect --format='{{"{{"}}.State.ExitCode{{"}}"}}' "scrapix-init-topics" 2>/dev/null || echo "1")
            if [ "$exit_code" = "0" ]; then
                echo "done"
                break
            else
                echo "FAILED (exit code $exit_code)"
                exit 1
            fi
        fi
        sleep 1
        timeout=$((timeout - 1))
    done
    if [ $timeout -eq 0 ]; then
        echo "TIMEOUT"
        exit 1
    fi
    echo "All infrastructure ready!"
