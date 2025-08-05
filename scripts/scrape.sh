#!/bin/bash
# Scraper wrapper script that preserves working directory for relative paths

# Get the directory where this script is located (project root)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Store original working directory
ORIGINAL_CWD="$(pwd)"

# Export it so the CLI can access it
export INIT_CWD="$ORIGINAL_CWD"

# Pass through CRAWLEE_STORAGE_DIR if set, otherwise use current directory
if [ -z "$CRAWLEE_STORAGE_DIR" ]; then
    export CRAWLEE_STORAGE_DIR="$PROJECT_ROOT/apps/scraper/cli/storage"
    mkdir -p "$CRAWLEE_STORAGE_DIR/request_queues/default"
    mkdir -p "$CRAWLEE_STORAGE_DIR/key_value_stores/default"
fi

# Build core package
(cd "$PROJECT_ROOT/apps/scraper/core" && yarn build)

# Build and run CLI with all arguments passed through
(cd "$PROJECT_ROOT/apps/scraper/cli" && yarn build && yarn start "$@")