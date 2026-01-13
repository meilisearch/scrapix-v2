# =============================================================================
# Scrapix Multi-Stage Dockerfile
# =============================================================================
# Builds all Rust binaries
# =============================================================================

# -----------------------------------------------------------------------------
# Stage 1: Builder - Build all binaries
# -----------------------------------------------------------------------------
FROM rust:1.92-bookworm AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    cmake \
    libssl-dev \
    libsasl2-dev \
    libclang-dev \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Copy source and build
COPY . .
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
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 -s /bin/bash scrapix
USER scrapix
WORKDIR /app

# -----------------------------------------------------------------------------
# Stage 3a: API Service
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-api
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-api /app/scrapix-api
EXPOSE 8080
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-api"]

# -----------------------------------------------------------------------------
# Stage 3b: Frontier Service
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-frontier-service
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-frontier-service /app/scrapix-frontier-service
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-frontier-service"]

# -----------------------------------------------------------------------------
# Stage 3c: Crawler Worker
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-worker-crawler
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-worker-crawler /app/scrapix-worker-crawler
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-worker-crawler"]

# -----------------------------------------------------------------------------
# Stage 3d: Content Worker
# -----------------------------------------------------------------------------
FROM runtime-base AS scrapix-worker-content
COPY --from=builder --chown=scrapix:scrapix /app/target/release/scrapix-worker-content /app/scrapix-worker-content
ENV RUST_LOG=info
ENTRYPOINT ["/app/scrapix-worker-content"]

# -----------------------------------------------------------------------------
# Stage 3e: CLI
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
