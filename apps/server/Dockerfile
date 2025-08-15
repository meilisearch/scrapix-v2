# Build from root context: docker build -f apps/server/Dockerfile .
FROM ghcr.io/puppeteer/puppeteer:23.9.0 AS builder

USER root
WORKDIR /app

# Copy all necessary config files from root
COPY tsconfig.base.json ./

# Copy workspace files
COPY apps/core/package.json ./apps/core/
COPY apps/server/package.json ./apps/server/

# Copy all source code
COPY apps/core/ ./apps/core/
COPY apps/server/ ./apps/server/

# Install dependencies for each package individually
WORKDIR /app/apps/core
RUN npm install --legacy-peer-deps

WORKDIR /app/apps/server
RUN npm install --legacy-peer-deps

# Build core and server
WORKDIR /app/apps/core
RUN npm run build

WORKDIR /app/apps/server
RUN npm run build

# Production stage - simpler approach: copy everything from builder
FROM ghcr.io/puppeteer/puppeteer:23.9.0

USER root
WORKDIR /app

# Copy everything from builder (includes node_modules and built files)
COPY --from=builder /app /app

# Prune dev dependencies for production
WORKDIR /app/apps/core
RUN npm prune --production

WORKDIR /app/apps/server
RUN npm prune --production

# Create app user and set permissions
RUN groupadd -r appuser && useradd -r -g appuser appuser && \
    chown -R appuser:appuser /app

USER appuser

# Environment variables
ENV NODE_ENV=production
ENV PUPPETEER_SKIP_CHROMIUM_DOWNLOAD=true
ENV PUPPETEER_EXECUTABLE_PATH=/usr/bin/google-chrome-stable

EXPOSE 8080

# Start the server
WORKDIR /app/apps/server
CMD ["node", "dist/index.js"]