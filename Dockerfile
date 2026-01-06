# =============================================================================
# Scrapix Multi-Stage Dockerfile
# =============================================================================
# Builds all Rust binaries using cargo-chef for efficient layer caching
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Chef - Prepare recipe for dependency caching
# -----------------------------------------------------------------------------
FROM rust:1.92-bullseye AS chef
RUN cargo install cargo-chef
WORKDIR /app

# -----------------------------------------------------------------------------
# Stage 2: Planner - Generate the recipe.json
# -----------------------------------------------------------------------------
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# -----------------------------------------------------------------------------
# Stage 3: Builder - Build dependencies and application
# -----------------------------------------------------------------------------
FROM chef AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    libssl-dev \
    libsasl2-dev \
    libclang-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Build dependencies (cached layer)
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --workspace

# -----------------------------------------------------------------------------
# Stage 4: Runtime base - Minimal runtime image
# -----------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime-base

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libsasl2-2 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 -s /bin/bash scrapix
USER scrapix
WORKDIR /app

# -----------------------------------------------------------------------------
# Stage 5a: API Service
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-api
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-api /app/scrapix-api
EXPOSE 8080
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-api"]

# -----------------------------------------------------------------------------
# Stage 5b: Frontier Service
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-frontier-service
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-frontier-service /app/scrapix-frontier-service
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-frontier-service"]

# -----------------------------------------------------------------------------
# Stage 5c: Crawler Worker
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-worker-crawler
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-worker-crawler /app/scrapix-worker-crawler
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-worker-crawler"]

# -----------------------------------------------------------------------------
# Stage 5d: Content Worker
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-worker-content
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-worker-content /app/scrapix-worker-content
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-worker-content"]

# -----------------------------------------------------------------------------
# Stage 5e: CLI
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-cli
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/scrapix
ENTRYPOINT ["/app/scrapix"]

# -----------------------------------------------------------------------------
# Default target: All binaries in one image (for development)
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-all
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-api /app/
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-frontier-service /app/
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-worker-crawler /app/
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-worker-content /app/
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix /app/
