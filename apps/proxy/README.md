# @scrapix/proxy

HTTP/HTTPS proxy server for distributed web crawling with authentication and management features.

## Overview

The proxy server provides a centralized proxy solution for web crawling operations, supporting authentication, request routing, and connection management. It's designed for deployment across multiple regions for distributed crawling.

## Features

- 🌐 HTTP/HTTPS proxy support
- 🔐 Optional authentication (Basic Auth & Token)
- 📊 Connection statistics and monitoring
- 🚦 Connection pooling and management
- 🏥 Health check endpoints
- 🌍 Multi-region deployment support

## Architecture

The proxy runs two services:
1. **Management API** (Port 3000) - HTTP API for health checks and statistics
2. **Proxy Service** (Port 8080) - The actual proxy server

## API Endpoints

### Management API (Port 3000)

#### `GET /proxy-health`
Health check endpoint for monitoring proxy status.

**Response:**
```json
{
  "status": "healthy",
  "uptime": 3600,
  "connections": {
    "active": 5,
    "total": 150
  },
  "version": "0.1.0"
}
```

#### `GET /proxy-stats`
Get detailed proxy statistics.

**Response:**
```json
{
  "requests": {
    "total": 1000,
    "successful": 950,
    "failed": 50
  },
  "bandwidth": {
    "bytes_sent": 10485760,
    "bytes_received": 52428800
  },
  "connections": {
    "current": 5,
    "peak": 25,
    "total": 150
  },
  "uptime": 3600
}
```

#### `GET /proxy-connections`
List active proxy connections.

**Response:**
```json
{
  "connections": [
    {
      "id": "conn_123",
      "client_ip": "192.168.1.1",
      "target_host": "example.com",
      "started_at": "2024-01-01T00:00:00.000Z",
      "bytes_transferred": 1024
    }
  ]
}
```

### Proxy Service (Port 8080)

The proxy service handles HTTP/HTTPS requests. Configure your crawler to use:
```
http://proxy-host:8080
```

With authentication:
```
http://username:password@proxy-host:8080
```

## Authentication

The proxy supports three authentication modes:

### 1. No Authentication (default)
```bash
# No environment variables needed
```

### 2. Basic Authentication
```bash
PROXY_AUTH_ENABLED=true
PROXY_AUTH_USERNAME=admin
PROXY_AUTH_PASSWORD=secret
```

Client usage:
```javascript
{
  proxy: {
    host: 'proxy-host',
    port: 8080,
    auth: {
      username: 'admin',
      password: 'secret'
    }
  }
}
```

### 3. Token Authentication
```bash
PROXY_AUTH_ENABLED=true
PROXY_AUTH_TOKEN=your-secret-token
```

Client usage with header:
```javascript
{
  headers: {
    'Proxy-Authorization': 'Bearer your-secret-token'
  }
}
```

## Environment Variables

```bash
# Proxy Configuration
PORT=3000                    # Management API port
PROXY_PORT=8080             # Proxy service port
NODE_ENV=production         # Environment

# Authentication (optional)
PROXY_AUTH_ENABLED=false   # Enable authentication
PROXY_AUTH_USERNAME=admin  # Basic auth username
PROXY_AUTH_PASSWORD=secret # Basic auth password
PROXY_AUTH_TOKEN=token     # Bearer token

# Logging
LOG_LEVEL=info             # Log level (debug/info/warn/error)
```

## Running the Proxy

### Development
```bash
yarn dev   # Run with hot-reload
```

### Production
```bash
yarn build  # Build TypeScript
yarn start  # Start proxy
```

### Docker
```bash
# Build from project root
docker build -f apps/proxy/Dockerfile -t scrapix-proxy .

# Run container
docker run -p 3000:3000 -p 8080:8080 \
  -e PROXY_AUTH_ENABLED=true \
  -e PROXY_AUTH_TOKEN=secret \
  scrapix-proxy
```

### Deployment to Fly.io
```bash
# Single region deployment
yarn deploy:proxy

# Multi-region deployment
yarn deploy:proxy:regions
```

## Usage Examples

### With curl
```bash
# Without authentication
curl -x http://localhost:8080 https://example.com

# With basic auth
curl -x http://admin:secret@localhost:8080 https://example.com

# With token auth
curl -x http://localhost:8080 \
  -H "Proxy-Authorization: Bearer your-token" \
  https://example.com
```

### With Node.js
```javascript
const axios = require('axios');

// Configure axios to use proxy
const response = await axios.get('https://example.com', {
  proxy: {
    host: 'localhost',
    port: 8080,
    auth: {
      username: 'admin',
      password: 'secret'
    }
  }
});
```

### With Puppeteer
```javascript
const browser = await puppeteer.launch({
  args: [
    '--proxy-server=http://localhost:8080'
  ]
});

// With authentication
const page = await browser.newPage();
await page.authenticate({
  username: 'admin',
  password: 'secret'
});
```

## Multi-Region Deployment

Deploy proxies across multiple regions for distributed crawling:

```bash
# Deploy to all regions
yarn deploy:proxy:regions

# Check status
yarn deploy:proxy:status

# Destroy all regional deployments
yarn deploy:proxy:destroy
```

Default regions:
- iad (Virginia)
- lax (Los Angeles)
- lhr (London)
- fra (Frankfurt)
- nrt (Tokyo)
- syd (Sydney)

## Performance Tuning

### Connection Limits
```javascript
// Maximum concurrent connections
MAX_CONNECTIONS = 1000

// Connection timeout (ms)
CONNECTION_TIMEOUT = 30000

// Keep-alive timeout (ms)
KEEPALIVE_TIMEOUT = 60000
```

### Memory Management
The proxy uses streaming to handle large responses efficiently without loading entire payloads into memory.

## Security Considerations

1. **Always use authentication in production**
2. **Use HTTPS for management API in production**
3. **Implement IP whitelisting if possible**
4. **Rotate authentication tokens regularly**
5. **Monitor for unusual traffic patterns**

## Monitoring

The proxy exposes metrics for monitoring:

- Active connections
- Request success/failure rates
- Bandwidth usage
- Response times
- Error rates

Use the `/proxy-stats` endpoint to collect metrics for your monitoring system.