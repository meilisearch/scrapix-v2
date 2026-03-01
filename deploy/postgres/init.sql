-- Scrapix PostgreSQL Schema
-- Runs automatically on first boot via docker-entrypoint-initdb.d

-- Enable UUID generation
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ============================================================================
-- Users (replaces Supabase auth.users + profiles)
-- ============================================================================

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    full_name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_users_email ON users (email);

-- ============================================================================
-- Accounts (billing entities)
-- ============================================================================

CREATE TABLE accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    tier TEXT NOT NULL DEFAULT 'free' CHECK (tier IN ('free', 'starter', 'pro', 'enterprise')),
    active BOOLEAN NOT NULL DEFAULT true,
    stripe_customer_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================================
-- Account Members (user <-> account join table)
-- ============================================================================

CREATE TABLE account_members (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'owner' CHECK (role IN ('owner', 'admin', 'member')),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, account_id)
);

CREATE INDEX idx_account_members_user_id ON account_members (user_id);
CREATE INDEX idx_account_members_account_id ON account_members (account_id);

-- ============================================================================
-- API Keys
-- ============================================================================

CREATE TABLE api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_api_keys_account_id ON api_keys (account_id);
CREATE INDEX idx_api_keys_key_hash ON api_keys (key_hash);

-- ============================================================================
-- validate_api_key function (used by the Rust API auth middleware)
-- ============================================================================

CREATE OR REPLACE FUNCTION validate_api_key(p_key_hash TEXT)
RETURNS TABLE (account_id UUID, tier TEXT, active BOOLEAN) AS $$
BEGIN
    RETURN QUERY
    SELECT a.id AS account_id, a.tier, a.active
    FROM api_keys k
    JOIN accounts a ON a.id = k.account_id
    WHERE k.key_hash = p_key_hash
      AND k.active = true
      AND a.active = true;

    -- Update last_used_at
    UPDATE api_keys SET last_used_at = now() WHERE api_keys.key_hash = p_key_hash AND api_keys.active = true;
END;
$$ LANGUAGE plpgsql;

-- ============================================================================
-- Updated-at trigger
-- ============================================================================

CREATE OR REPLACE FUNCTION update_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trg_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

CREATE TRIGGER trg_accounts_updated_at
    BEFORE UPDATE ON accounts
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

-- ============================================================================
-- Saved Crawl Configs (with optional cron scheduling)
-- ============================================================================

CREATE TABLE crawl_configs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    description TEXT,
    config JSONB NOT NULL,
    cron_expression TEXT,
    cron_enabled BOOLEAN NOT NULL DEFAULT false,
    last_run_at TIMESTAMPTZ,
    next_run_at TIMESTAMPTZ,
    last_job_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, name)
);

CREATE INDEX idx_crawl_configs_account_id ON crawl_configs (account_id);
CREATE INDEX idx_crawl_configs_next_run ON crawl_configs (next_run_at)
    WHERE cron_enabled = true AND cron_expression IS NOT NULL;

CREATE TRIGGER trg_crawl_configs_updated_at
    BEFORE UPDATE ON crawl_configs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at();

-- ============================================================================
-- Jobs (persistent crawl job state)
-- ============================================================================

CREATE TABLE jobs (
    job_id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'completed', 'failed', 'cancelled', 'paused')),
    index_uid TEXT NOT NULL,
    account_id UUID REFERENCES accounts(id) ON DELETE SET NULL,
    pages_crawled BIGINT NOT NULL DEFAULT 0,
    pages_indexed BIGINT NOT NULL DEFAULT 0,
    documents_sent BIGINT NOT NULL DEFAULT 0,
    errors BIGINT NOT NULL DEFAULT 0,
    bytes_downloaded BIGINT NOT NULL DEFAULT 0,
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    crawl_rate DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    eta_seconds BIGINT,
    error_message TEXT,
    start_urls JSONB NOT NULL DEFAULT '[]',
    max_pages BIGINT,
    config JSONB,
    swap_temp_index TEXT,
    swap_meilisearch_url TEXT,
    swap_meilisearch_api_key TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_jobs_account_id ON jobs (account_id);
CREATE INDEX idx_jobs_status ON jobs (status);
CREATE INDEX idx_jobs_created_at ON jobs (created_at DESC);
CREATE INDEX idx_jobs_active ON jobs (job_id) WHERE status IN ('pending', 'running', 'paused');

CREATE TRIGGER trg_jobs_updated_at
    BEFORE UPDATE ON jobs FOR EACH ROW EXECUTE FUNCTION update_updated_at();
