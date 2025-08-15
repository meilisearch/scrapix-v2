# Build from root context: docker build -f apps/proxy/Dockerfile .
FROM node:18-alpine AS builder

# Install basic dependencies
RUN apk add --no-cache \
    ca-certificates

WORKDIR /app

# Copy root package files and configs
COPY package.json yarn.lock ./
COPY tsconfig.base.json ./

# Copy proxy package.json
COPY apps/proxy/package.json ./apps/proxy/

# Install all dependencies (yarn workspaces will handle it)
RUN yarn install --frozen-lockfile

# Copy proxy source code
COPY apps/proxy/ ./apps/proxy/

# Build the proxy
WORKDIR /app/apps/proxy
RUN yarn build

# Production stage
FROM node:18-alpine

RUN apk add --no-cache \
    ca-certificates

WORKDIR /app

# Copy built proxy app
COPY --from=builder /app/apps/proxy/dist ./dist
COPY --from=builder /app/apps/proxy/package.json ./

# Install only production dependencies
RUN yarn install --production --frozen-lockfile

# Expose ports
EXPOSE 3000 8080

# Start the application
CMD ["node", "dist/index.js"]