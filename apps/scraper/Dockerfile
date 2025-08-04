# Use the official Puppeteer image which has Chrome pre-installed
FROM ghcr.io/puppeteer/puppeteer:23.9.0 AS builder

USER root
WORKDIR /app

# Copy package files
COPY package.json ./
COPY core/package.json ./core/
COPY server/package.json ./server/
COPY cli/package.json ./cli/

# Copy all source code first
COPY . .

# Install dependencies for each package individually
WORKDIR /app/core
RUN npm install --legacy-peer-deps

WORKDIR /app/server
RUN npm install --legacy-peer-deps

# Build core and server using standalone configs
WORKDIR /app/core
RUN npx tsc --project tsconfig.standalone.json

WORKDIR /app/server  
RUN npx tsc --project tsconfig.standalone.json

# Production stage - use same base for consistency
FROM ghcr.io/puppeteer/puppeteer:23.9.0

USER root
WORKDIR /app

# Copy everything from builder
COPY --from=builder /app /app

# Prune dev dependencies for production
WORKDIR /app/core
RUN npm prune --production

WORKDIR /app/server
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
WORKDIR /app/server
CMD ["node", "dist/index.js"]