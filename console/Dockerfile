FROM node:24-alpine AS base

# Stage 1: Install dependencies
FROM base AS deps
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci

# Stage 2: Build the application
FROM base AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
ENV NEXT_TELEMETRY_DISABLED=1

# Next.js inlines NEXT_PUBLIC_* vars at build time. In Docker builds
# (e.g. Heroku), runtime config vars aren't available during build,
# so we hardcode defaults here. The publishable key is public by design.
ENV NEXT_PUBLIC_STRIPE_PUBLISHABLE_KEY=pk_test_51TAwQdRGrCEv0wspTJFC1nDzrfhVwg2pxBtfAplfdjdTDkB8kOxrVAodNPYjehdMhIzTCY4HUnJ6I21ELXuKiXDq00pDyrnwvj
ENV NEXT_PUBLIC_SCRAPIX_API_URL=https://api.scrapix.meilisearch.com

RUN npm run build

# Stage 3: Production runner
FROM base AS runner
WORKDIR /app
ENV NODE_ENV=production
ENV NEXT_TELEMETRY_DISABLED=1

RUN addgroup --system --gid 1001 nodejs
RUN adduser --system --uid 1001 nextjs

COPY --from=builder /app/public ./public

# Next.js standalone output
COPY --from=builder --chown=nextjs:nodejs /app/.next/standalone ./
COPY --from=builder --chown=nextjs:nodejs /app/.next/static ./.next/static

USER nextjs
EXPOSE 3000
ENV HOSTNAME=0.0.0.0

CMD ["node", "server.js"]
