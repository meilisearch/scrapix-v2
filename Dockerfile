# =============================================================================
# Scrapix Multi-Stage Dockerfile
# =============================================================================
# Builds all Rust binaries with dependency caching for fast rebuilds.
#
# Layer strategy:
#   1. Copy Cargo manifests + lock file, create stub sources
#   2. `cargo build --release` to compile all dependencies (cached)
#   3. Copy real source, build again (only recompiles workspace crates)
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Builder - Build all binaries
# -----------------------------------------------------------------------------
FROM rust:1.93-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    libssl-dev \
    libsasl2-dev \
    libclang-dev \
    libcurl4-openssl-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace manifest and lock file
COPY Cargo.toml Cargo.lock ./

# Copy all crate manifests
COPY crates/scrapix-core/Cargo.toml crates/scrapix-core/Cargo.toml
COPY crates/scrapix-frontier/Cargo.toml crates/scrapix-frontier/Cargo.toml
COPY crates/scrapix-crawler/Cargo.toml crates/scrapix-crawler/Cargo.toml
COPY crates/scrapix-parser/Cargo.toml crates/scrapix-parser/Cargo.toml
COPY crates/scrapix-extractor/Cargo.toml crates/scrapix-extractor/Cargo.toml
COPY crates/scrapix-ai/Cargo.toml crates/scrapix-ai/Cargo.toml
COPY crates/scrapix-storage/Cargo.toml crates/scrapix-storage/Cargo.toml
COPY crates/scrapix-queue/Cargo.toml crates/scrapix-queue/Cargo.toml
COPY crates/scrapix-telemetry/Cargo.toml crates/scrapix-telemetry/Cargo.toml
COPY bins/scrapix-api/Cargo.toml bins/scrapix-api/Cargo.toml
COPY bins/scrapix-worker-crawler/Cargo.toml bins/scrapix-worker-crawler/Cargo.toml
COPY bins/scrapix-worker-content/Cargo.toml bins/scrapix-worker-content/Cargo.toml
COPY bins/scrapix-frontier-service/Cargo.toml bins/scrapix-frontier-service/Cargo.toml
COPY bins/scrapix-cli/Cargo.toml bins/scrapix-cli/Cargo.toml
COPY bins/scrapix/Cargo.toml bins/scrapix/Cargo.toml
COPY benches/Cargo.toml benches/Cargo.toml
COPY tests/Cargo.toml tests/Cargo.toml

# Create stub source files so cargo can resolve the workspace and compile deps
RUN mkdir -p crates/scrapix-core/src && echo "" > crates/scrapix-core/src/lib.rs \
    && mkdir -p crates/scrapix-frontier/src && echo "" > crates/scrapix-frontier/src/lib.rs \
    && mkdir -p crates/scrapix-crawler/src && echo "" > crates/scrapix-crawler/src/lib.rs \
    && mkdir -p crates/scrapix-parser/src && echo "" > crates/scrapix-parser/src/lib.rs \
    && mkdir -p crates/scrapix-extractor/src && echo "" > crates/scrapix-extractor/src/lib.rs \
    && mkdir -p crates/scrapix-ai/src && echo "" > crates/scrapix-ai/src/lib.rs \
    && mkdir -p crates/scrapix-storage/src && echo "" > crates/scrapix-storage/src/lib.rs \
    && mkdir -p crates/scrapix-queue/src && echo "" > crates/scrapix-queue/src/lib.rs \
    && mkdir -p crates/scrapix-telemetry/src && echo "" > crates/scrapix-telemetry/src/lib.rs \
    && mkdir -p bins/scrapix-api/src && echo "" > bins/scrapix-api/src/lib.rs && echo "fn main() {}" > bins/scrapix-api/src/main.rs \
    && mkdir -p bins/scrapix-worker-crawler/src && echo "" > bins/scrapix-worker-crawler/src/lib.rs && echo "fn main() {}" > bins/scrapix-worker-crawler/src/main.rs \
    && mkdir -p bins/scrapix-worker-content/src && echo "" > bins/scrapix-worker-content/src/lib.rs && echo "fn main() {}" > bins/scrapix-worker-content/src/main.rs \
    && mkdir -p bins/scrapix-frontier-service/src && echo "" > bins/scrapix-frontier-service/src/lib.rs && echo "fn main() {}" > bins/scrapix-frontier-service/src/main.rs \
    && mkdir -p bins/scrapix-cli/src && echo "" > bins/scrapix-cli/src/lib.rs && echo "fn main() {}" > bins/scrapix-cli/src/main.rs \
    && mkdir -p bins/scrapix/src && echo "fn main() {}" > bins/scrapix/src/main.rs \
    && mkdir -p benches/src && echo "" > benches/src/lib.rs \
    && mkdir -p tests/src && echo "" > tests/src/lib.rs

# Build dependencies only (this layer is cached until Cargo.toml/Cargo.lock change)
RUN cargo build --release --workspace 2>&1 || true

# Remove stub artifacts so cargo detects the real source as changed
RUN find target/release/.fingerprint \
    -name "scrapix-*" -type d -exec rm -rf {} + 2>/dev/null || true

# Copy real source
COPY crates/ crates/
COPY bins/ bins/
COPY benches/ benches/
COPY tests/ tests/

# Build workspace (only recompiles workspace crates, deps are cached)
RUN cargo build --release --workspace

# -----------------------------------------------------------------------------
# Stage 2: Runtime base - Minimal runtime image
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime-base

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libsasl2-2 \
    libcurl4 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 -s /bin/bash scrapix
USER scrapix
WORKDIR /app

# -----------------------------------------------------------------------------
# Stage 3a: API Service
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-api
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/scrapix
EXPOSE 8080
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix", "api"]

# -----------------------------------------------------------------------------
# Stage 3b: Frontier Service
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-frontier-service
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/scrapix
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix", "frontier"]

# -----------------------------------------------------------------------------
# Stage 3c: Crawler Worker
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-worker-crawler
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/scrapix
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix", "crawler"]

# -----------------------------------------------------------------------------
# Stage 3d: Content Worker
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-worker-content
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/scrapix
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix", "content"]

# -----------------------------------------------------------------------------
# Stage 3e: CLI
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-cli
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/scrapix
ENTRYPOINT ["/app/scrapix"]

# -----------------------------------------------------------------------------
# Default target: Single unified binary (runs all services or any subcommand)
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-all
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/scrapix
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix", "all"]
