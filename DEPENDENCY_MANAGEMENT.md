# Dependency Management Strategy

## Overview
This Turborepo uses Yarn workspaces to manage dependencies across packages. Common dependencies are hoisted to the root to ensure consistency.

## Shared Dependencies (Root package.json)

### Production Dependencies
- `dotenv`: ^16.0.0 - Environment variable management
- `meilisearch`: 0.45.0 - Search engine client
- `uuid`: ^11.1.0 - UUID generation
- `zod`: ^3.23.8 - Schema validation

### Development Dependencies
- **TypeScript & Linting**: typescript, eslint, prettier and related plugins
- **Testing**: jest, ts-jest, @types/jest
- **Development Tools**: nodemon, concurrently, ts-node
- **Type Definitions**: @types/node, @types/express, @types/uuid, @types/yargs

## Package-Specific Dependencies

### @scrapix/core
- Crawling: crawlee, puppeteer, playwright, cheerio
- OpenTelemetry: All observability packages
- Data Processing: pdf-parse, node-html-markdown, fast-xml-parser

### @scrapix/server
- Web Framework: express ^5.0.0, express-rate-limit
- Job Queue: bull
- Database: @supabase/supabase-js

### @scrapix/cli
- CLI Framework: yargs
- Crawling: crawlee (direct dependency for CLI usage)

### @scrapix/proxy
- Web Framework: express ^5.0.0
- Proxy: http-proxy
- CORS: cors

## Guidelines

1. **Adding New Dependencies**
   - If used by multiple packages → Add to root package.json
   - If package-specific → Add to the specific package
   - Always check if dependency already exists at root level

2. **Version Management**
   - Use exact versions for critical dependencies (e.g., meilisearch)
   - Use caret (^) for flexible dependencies
   - Keep all packages on the same major version of shared deps

3. **Type Definitions**
   - All @types/* packages should be in root devDependencies
   - Ensures consistent typing across the monorepo

4. **Updates**
   - Run `yarn install` after any package.json changes
   - Use `yarn upgrade-interactive` to update dependencies
   - Test thoroughly after major version updates

## Common Commands

```bash
# Install all dependencies
yarn install

# Add dependency to root
yarn add <package>

# Add dependency to specific workspace
yarn workspace @scrapix/core add <package>

# Check for outdated packages
yarn outdated

# Upgrade interactive
yarn upgrade-interactive
```

## Resolutions
The root package.json includes resolutions for:
- `form-data`: ^4.0.4 - Ensures consistent version across all dependencies