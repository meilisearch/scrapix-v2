# Scrapix Implementation Roadmap

## Current Status: MVP Complete

The core distributed crawling system is fully implemented with ~24,000 lines of Rust code across 9 library crates and 5 binaries.

---

## Remaining Work

### 1. Integration Tests
**Priority: High** | **Effort: Medium**

Create end-to-end integration tests for:
- [ ] Full crawl pipeline (URL → Fetch → Parse → Index)
- [ ] Frontier deduplication and politeness
- [ ] API endpoints with mocked Kafka
- [ ] Content worker document processing

**Files to create:**
```
tests/
├── integration/
│   ├── mod.rs
│   ├── crawl_pipeline.rs
│   ├── frontier_service.rs
│   └── api_endpoints.rs
└── common/
    └── mod.rs  # Test fixtures and helpers
```

---

### 2. API Authentication
**Priority: High** | **Effort: Medium**

Implement authentication layer for the REST API:
- [ ] API key authentication (header-based)
- [ ] JWT token support (optional)
- [ ] Rate limiting per API key
- [ ] RBAC for admin vs user endpoints

**Files to create:**
```
bins/scrapix-api/src/
├── auth.rs         # Auth middleware and extractors
├── middleware.rs   # Rate limiting, logging
└── routes/
    └── admin.rs    # Protected admin endpoints
```

---

### 3. WebSocket Real-time Events
**Priority: Medium** | **Effort: Low**

Add WebSocket support alongside existing SSE:
- [ ] WebSocket handler for job events
- [ ] Bi-directional communication for job control
- [ ] Connection management and heartbeats

**Files to create:**
```
bins/scrapix-api/src/websocket.rs
```

---

### 4. ScyllaDB Storage (Scale Phase)
**Priority: Low** | **Effort: High**

For Phase 3+ scaling (100M+ pages/day):
- [ ] ScyllaDB client implementation
- [ ] URL state persistence
- [ ] Crawl history storage
- [ ] Migration from Meilisearch-only

**Files to create:**
```
crates/scrapix-storage/src/scylla.rs
```

---

### 5. AI Enrichment Integration ✅ DONE
**Priority: High** | **Effort: Medium**

Wire `scrapix-ai` crate into the content processing pipeline:
- [x] AI client (OpenAI/Anthropic) - exists in `scrapix-ai/src/client.rs`
- [x] Extraction module - exists in `scrapix-ai/src/extraction.rs`
- [x] Summary module - exists in `scrapix-ai/src/summary.rs`
- [x] Embedding module - exists in `scrapix-ai/src/embedding.rs`
- [x] **Wire into content worker**
- [x] **Add CLI flags for AI processing**
- [x] Embeddings handled by Meilisearch auto-embeddings

**Usage:**
```bash
# Enable AI summarization
scrapix-worker-content --enable-summary --openai-api-key $OPENAI_API_KEY

# Enable AI extraction with custom prompt
scrapix-worker-content --enable-extraction \
  --extraction-prompt "Extract: title, author, date, topics as JSON" \
  --openai-api-key $OPENAI_API_KEY

# Both features
scrapix-worker-content --enable-summary --enable-extraction \
  --extraction-prompt "..." --openai-api-key $OPENAI_API_KEY
```

---

### 6. Embeddings & Vector Search
**Priority: Medium** | **Effort: Medium**

Connect embedding generation to Meilisearch:
- [ ] Configure Meilisearch vector settings
- [ ] Store embeddings with documents
- [ ] Add vector search API endpoint

---

### 7. Documentation
**Priority: Medium** | **Effort: Low**

Create user-facing documentation:
- [ ] `docs/api.md` - REST API reference
- [ ] `docs/configuration.md` - Config schema guide
- [ ] `docs/deployment.md` - K8s deployment guide
- [ ] `docs/quickstart.md` - Getting started

---

### 8. Webhooks Implementation
**Priority: Low** | **Effort: Medium**

Full webhook support in workers:
- [ ] Webhook dispatcher service
- [ ] Event filtering by type
- [ ] Retry logic with exponential backoff
- [ ] HMAC signature verification

---

### 9. Browser Pool Management
**Priority: Low** | **Effort: Medium**

Improve CDP renderer for high concurrency:
- [ ] Browser instance recycling
- [ ] Health monitoring and auto-restart
- [ ] Memory limit enforcement
- [ ] Page crash recovery

---

### 10. CLI Distributed Commands
**Priority: Medium** | **Effort: Low**

Add CLI commands for distributed crawling:
- [ ] `scrapix crawl submit` - Submit job to API
- [ ] `scrapix crawl status` - Check job status
- [ ] `scrapix crawl cancel` - Cancel running job
- [ ] `scrapix crawl list` - List all jobs

---

## Implementation Order (Recommended)

1. **AI Enrichment Integration** ← Now
2. Integration Tests
3. API Authentication
4. Documentation
5. CLI Distributed Commands
6. WebSocket Events
7. Webhooks
8. Browser Pool Management
9. ScyllaDB (when scaling)
