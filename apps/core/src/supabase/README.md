# Supabase Integration

Persistent storage layer for crawler configurations, run history, and event tracking using Supabase.

## Overview

This module provides optional persistent storage capabilities using Supabase as a backend. It handles configuration management, run tracking, and event storage.

## Architecture

```
supabase/
├── client.ts          # Supabase client singleton
├── config-manager.ts  # Configuration CRUD operations
├── run-manager.ts     # Crawler run tracking
└── event-storage.ts   # Event persistence
```

## Components

### Supabase Client

Singleton client for database operations:

```typescript
import { getSupabaseClient } from './client';

const supabase = getSupabaseClient({
  url: process.env.SUPABASE_URL,
  anonKey: process.env.SUPABASE_ANON_KEY
});
```

### Configuration Manager

Manage crawler configurations:

```typescript
import { ConfigManager } from './config-manager';

const configManager = new ConfigManager(supabase);

// Save configuration
const config = await configManager.create({
  name: 'Production Crawler',
  config: {
    start_urls: ['https://example.com'],
    crawler_type: 'puppeteer',
    features: { /* ... */ }
  }
});

// List configurations
const configs = await configManager.list({
  limit: 10,
  offset: 0
});

// Get specific config
const config = await configManager.get('config_id');

// Update configuration
await configManager.update('config_id', {
  name: 'Updated Config',
  config: { /* ... */ }
});

// Delete configuration
await configManager.delete('config_id');
```

### Run Manager

Track crawler execution history:

```typescript
import { RunManager } from './run-manager';

const runManager = new RunManager(supabase);

// Start a new run
const run = await runManager.startRun({
  config_id: 'config_123',
  started_by: 'user_456'
});

// Update run progress
await runManager.updateProgress(run.id, {
  urls_crawled: 50,
  documents_sent: 45,
  current_url: 'https://example.com/page50'
});

// Complete run
await runManager.completeRun(run.id, {
  status: 'completed',
  stats: {
    total_urls: 100,
    total_documents: 95,
    total_errors: 5,
    duration: 120000
  }
});

// Get run history
const runs = await runManager.getHistory({
  config_id: 'config_123',
  status: 'completed',
  limit: 50
});
```

### Event Storage

Persist crawler events for analysis:

```typescript
import { EventStorage } from './event-storage';

const eventStorage = new EventStorage(supabase);

// Store event
await eventStorage.store({
  run_id: 'run_123',
  event_type: 'progress',
  data: {
    urls_crawled: 10,
    documents_sent: 9
  }
});

// Batch store events
await eventStorage.batchStore([
  { run_id: 'run_123', event_type: 'document', data: { /* ... */ } },
  { run_id: 'run_123', event_type: 'error', data: { /* ... */ } }
]);

// Query events
const events = await eventStorage.getEvents({
  run_id: 'run_123',
  event_type: 'error',
  from: new Date('2024-01-01'),
  to: new Date('2024-01-31')
});

// Aggregate events
const stats = await eventStorage.aggregate({
  run_id: 'run_123',
  group_by: 'event_type'
});
```

## Database Schema

### Configurations Table
```sql
CREATE TABLE configurations (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name VARCHAR(255) NOT NULL,
  config JSONB NOT NULL,
  created_at TIMESTAMPTZ DEFAULT NOW(),
  updated_at TIMESTAMPTZ DEFAULT NOW(),
  created_by UUID REFERENCES auth.users(id),
  is_active BOOLEAN DEFAULT true
);
```

### Runs Table
```sql
CREATE TABLE runs (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  config_id UUID REFERENCES configurations(id),
  status VARCHAR(50) NOT NULL,
  started_at TIMESTAMPTZ DEFAULT NOW(),
  completed_at TIMESTAMPTZ,
  stats JSONB,
  error JSONB,
  created_by UUID REFERENCES auth.users(id)
);
```

### Events Table
```sql
CREATE TABLE events (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  run_id UUID REFERENCES runs(id),
  event_type VARCHAR(50) NOT NULL,
  data JSONB NOT NULL,
  created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX idx_events_run_id ON events(run_id);
CREATE INDEX idx_events_type ON events(event_type);
CREATE INDEX idx_events_created_at ON events(created_at);
```

## Real-time Subscriptions

Subscribe to real-time updates:

```typescript
// Subscribe to run updates
const subscription = supabase
  .channel('runs')
  .on('postgres_changes', {
    event: 'UPDATE',
    schema: 'public',
    table: 'runs',
    filter: `id=eq.${runId}`
  }, (payload) => {
    console.log('Run updated:', payload.new);
  })
  .subscribe();

// Subscribe to new events
const eventSub = supabase
  .channel('events')
  .on('postgres_changes', {
    event: 'INSERT',
    schema: 'public',
    table: 'events',
    filter: `run_id=eq.${runId}`
  }, (payload) => {
    console.log('New event:', payload.new);
  })
  .subscribe();

// Cleanup
subscription.unsubscribe();
```

## Row Level Security (RLS)

Implement security policies:

```sql
-- Enable RLS
ALTER TABLE configurations ENABLE ROW LEVEL SECURITY;
ALTER TABLE runs ENABLE ROW LEVEL SECURITY;
ALTER TABLE events ENABLE ROW LEVEL SECURITY;

-- Configurations: Users can only see their own
CREATE POLICY "Users can view own configurations" ON configurations
  FOR SELECT USING (auth.uid() = created_by);

CREATE POLICY "Users can create configurations" ON configurations
  FOR INSERT WITH CHECK (auth.uid() = created_by);

-- Runs: Users can see runs from their configs
CREATE POLICY "Users can view runs" ON runs
  FOR SELECT USING (
    config_id IN (
      SELECT id FROM configurations 
      WHERE created_by = auth.uid()
    )
  );
```

## Error Handling

Graceful degradation when Supabase is unavailable:

```typescript
class ConfigManager {
  async get(id: string) {
    try {
      const { data, error } = await this.supabase
        .from('configurations')
        .select('*')
        .eq('id', id)
        .single();
      
      if (error) throw error;
      return data;
    } catch (error) {
      console.error('Failed to fetch config:', error);
      // Fall back to local storage or defaults
      return this.getLocalConfig(id);
    }
  }
}
```

## Migrations

Database migration management:

```sql
-- Migration: 001_initial_schema.sql
CREATE TABLE IF NOT EXISTS configurations ( /* ... */ );
CREATE TABLE IF NOT EXISTS runs ( /* ... */ );
CREATE TABLE IF NOT EXISTS events ( /* ... */ );

-- Migration: 002_add_indexes.sql
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_runs_status 
  ON runs(status);
CREATE INDEX CONCURRENTLY IF NOT EXISTS idx_runs_created_at 
  ON runs(created_at DESC);
```

## Environment Variables

```bash
# Required for Supabase integration
SUPABASE_URL=https://xxxxx.supabase.co
SUPABASE_ANON_KEY=eyJhbGc...
SUPABASE_SERVICE_KEY=eyJhbGc... # For admin operations

# Optional
SUPABASE_JWT_SECRET=your-jwt-secret
```

## Best Practices

1. **Use connection pooling** - Reuse client instances
2. **Implement retry logic** - Handle transient failures
3. **Batch operations** - Use batch inserts for events
4. **Index strategically** - Add indexes for common queries
5. **Clean up old data** - Implement data retention policies
6. **Use transactions** - Ensure data consistency
7. **Monitor usage** - Track API calls and storage