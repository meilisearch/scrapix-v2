-- Scrapix PostgreSQL Schema
-- Idempotent: safe to run on every startup (all statements use IF NOT EXISTS)

-- Enable UUID generation
CREATE EXTENSION IF NOT EXISTS "pgcrypto";

-- ============================================================================
-- Users (replaces Supabase auth.users + profiles)
-- ============================================================================

CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    full_name TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_users_email ON users (email);

-- ============================================================================
-- Accounts (billing entities)
-- ============================================================================

CREATE TABLE IF NOT EXISTS accounts (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL,
    tier TEXT NOT NULL DEFAULT 'free' CHECK (tier IN ('free', 'starter', 'pro', 'enterprise')),
    active BOOLEAN NOT NULL DEFAULT true,
    stripe_customer_id TEXT,
    stripe_default_payment_method_id TEXT,
    credits_balance BIGINT NOT NULL DEFAULT 100,
    auto_topup_enabled BOOLEAN NOT NULL DEFAULT false,
    auto_topup_amount BIGINT NOT NULL DEFAULT 5000,
    auto_topup_threshold BIGINT NOT NULL DEFAULT 500,
    monthly_spend_limit BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================================
-- Account Members (user <-> account join table)
-- ============================================================================

CREATE TABLE IF NOT EXISTS account_members (
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    role TEXT NOT NULL DEFAULT 'owner' CHECK (role IN ('owner', 'admin', 'member')),
    joined_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, account_id)
);

CREATE INDEX IF NOT EXISTS idx_account_members_user_id ON account_members (user_id);
CREATE INDEX IF NOT EXISTS idx_account_members_account_id ON account_members (account_id);

-- ============================================================================
-- API Keys
-- ============================================================================

CREATE TABLE IF NOT EXISTS api_keys (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    prefix TEXT NOT NULL,
    key_hash TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT true,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_api_keys_account_id ON api_keys (account_id);
CREATE INDEX IF NOT EXISTS idx_api_keys_key_hash ON api_keys (key_hash);

-- ============================================================================
-- validate_api_key function (used by the Rust API auth middleware)
-- ============================================================================

CREATE OR REPLACE FUNCTION validate_api_key(p_key_hash TEXT)
RETURNS TABLE (account_id UUID, tier TEXT, active BOOLEAN, api_key_id UUID) AS $$
BEGIN
    RETURN QUERY
    SELECT a.id AS account_id, a.tier, a.active, k.id AS api_key_id
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

-- Triggers use DO blocks for idempotent creation
DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'trg_users_updated_at') THEN
        CREATE TRIGGER trg_users_updated_at
            BEFORE UPDATE ON users
            FOR EACH ROW EXECUTE FUNCTION update_updated_at();
    END IF;
END $$;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'trg_accounts_updated_at') THEN
        CREATE TRIGGER trg_accounts_updated_at
            BEFORE UPDATE ON accounts
            FOR EACH ROW EXECUTE FUNCTION update_updated_at();
    END IF;
END $$;

-- ============================================================================
-- Saved Crawl Configs (with optional cron scheduling)
-- ============================================================================

CREATE TABLE IF NOT EXISTS crawl_configs (
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

CREATE INDEX IF NOT EXISTS idx_crawl_configs_account_id ON crawl_configs (account_id);
CREATE INDEX IF NOT EXISTS idx_crawl_configs_next_run ON crawl_configs (next_run_at)
    WHERE cron_enabled = true AND cron_expression IS NOT NULL;

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'trg_crawl_configs_updated_at') THEN
        CREATE TRIGGER trg_crawl_configs_updated_at
            BEFORE UPDATE ON crawl_configs
            FOR EACH ROW EXECUTE FUNCTION update_updated_at();
    END IF;
END $$;

-- ============================================================================
-- Jobs (persistent crawl job state)
-- ============================================================================

CREATE TABLE IF NOT EXISTS jobs (
    job_id TEXT PRIMARY KEY,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'completed', 'failed', 'cancelled', 'paused')),
    index_uid TEXT NOT NULL,
    account_id UUID REFERENCES accounts(id) ON DELETE SET NULL,
    api_key_id UUID REFERENCES api_keys(id) ON DELETE SET NULL,
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

CREATE INDEX IF NOT EXISTS idx_jobs_account_id ON jobs (account_id);
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs (status);
CREATE INDEX IF NOT EXISTS idx_jobs_created_at ON jobs (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_jobs_active ON jobs (job_id) WHERE status IN ('pending', 'running', 'paused');

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'trg_jobs_updated_at') THEN
        CREATE TRIGGER trg_jobs_updated_at
            BEFORE UPDATE ON jobs FOR EACH ROW EXECUTE FUNCTION update_updated_at();
    END IF;
END $$;

-- ============================================================================
-- Meilisearch Engines (saved Meilisearch instances)
-- ============================================================================

CREATE TABLE IF NOT EXISTS meilisearch_engines (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    is_default BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, name)
);

CREATE INDEX IF NOT EXISTS idx_meilisearch_engines_account_id ON meilisearch_engines (account_id);

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_trigger WHERE tgname = 'trg_meilisearch_engines_updated_at') THEN
        CREATE TRIGGER trg_meilisearch_engines_updated_at
            BEFORE UPDATE ON meilisearch_engines
            FOR EACH ROW EXECUTE FUNCTION update_updated_at();
    END IF;
END $$;

-- ============================================================================
-- Transactions (credit operations log)
-- ============================================================================

CREATE TABLE IF NOT EXISTS transactions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id UUID NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    type TEXT NOT NULL CHECK (type IN (
        'initial_deposit', 'manual_topup', 'auto_topup',
        'usage_deduction', 'refund', 'adjustment'
    )),
    amount BIGINT NOT NULL,
    balance_after BIGINT NOT NULL,
    description TEXT,
    metadata JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_transactions_account_id ON transactions (account_id);
CREATE INDEX IF NOT EXISTS idx_transactions_created_at ON transactions (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_transactions_account_created ON transactions (account_id, created_at DESC);

-- ============================================================================
-- OAuth 2.1 Clients (Dynamic Client Registration — RFC 7591)
-- ============================================================================

CREATE TABLE IF NOT EXISTS oauth_clients (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    client_id VARCHAR(64) UNIQUE NOT NULL,
    client_name VARCHAR(255),
    redirect_uris TEXT[] NOT NULL,
    scope VARCHAR(255) DEFAULT 'mcp',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================================
-- OAuth 2.1 Authorization Codes (short-lived, single-use)
-- ============================================================================

CREATE TABLE IF NOT EXISTS oauth_authorization_codes (
    code VARCHAR(128) PRIMARY KEY,
    client_id VARCHAR(64) NOT NULL REFERENCES oauth_clients(client_id),
    user_id UUID NOT NULL REFERENCES users(id),
    redirect_uri TEXT NOT NULL,
    scope VARCHAR(255) DEFAULT 'mcp',
    code_challenge VARCHAR(128) NOT NULL,
    code_challenge_method VARCHAR(10) NOT NULL DEFAULT 'S256',
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_oauth_codes_expires ON oauth_authorization_codes(expires_at);

-- ============================================================================
-- OAuth 2.1 Tokens (access + refresh, stored as SHA-256 hashes)
-- ============================================================================

CREATE TABLE IF NOT EXISTS oauth_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    token_hash VARCHAR(64) UNIQUE NOT NULL,
    token_type VARCHAR(16) NOT NULL CHECK (token_type IN ('access', 'refresh')),
    client_id VARCHAR(64) NOT NULL REFERENCES oauth_clients(client_id),
    user_id UUID NOT NULL REFERENCES users(id),
    scope VARCHAR(255) DEFAULT 'mcp',
    expires_at TIMESTAMPTZ NOT NULL,
    revoked BOOLEAN NOT NULL DEFAULT false,
    parent_token_id UUID REFERENCES oauth_tokens(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_oauth_tokens_hash ON oauth_tokens(token_hash) WHERE revoked = false;

-- ============================================================================
-- Idempotent migrations for existing databases
-- ============================================================================

DO $$ BEGIN
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'accounts' AND column_name = 'credits_balance') THEN
        ALTER TABLE accounts ADD COLUMN credits_balance BIGINT NOT NULL DEFAULT 100;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'accounts' AND column_name = 'auto_topup_enabled') THEN
        ALTER TABLE accounts ADD COLUMN auto_topup_enabled BOOLEAN NOT NULL DEFAULT false;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'accounts' AND column_name = 'auto_topup_amount') THEN
        ALTER TABLE accounts ADD COLUMN auto_topup_amount BIGINT NOT NULL DEFAULT 5000;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'accounts' AND column_name = 'auto_topup_threshold') THEN
        ALTER TABLE accounts ADD COLUMN auto_topup_threshold BIGINT NOT NULL DEFAULT 500;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'accounts' AND column_name = 'monthly_spend_limit') THEN
        ALTER TABLE accounts ADD COLUMN monthly_spend_limit BIGINT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'accounts' AND column_name = 'stripe_default_payment_method_id') THEN
        ALTER TABLE accounts ADD COLUMN stripe_default_payment_method_id TEXT;
    END IF;

    -- Email verification and notification preferences on users
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'users' AND column_name = 'email_verified') THEN
        ALTER TABLE users ADD COLUMN email_verified BOOLEAN NOT NULL DEFAULT false;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'users' AND column_name = 'email_verification_token') THEN
        ALTER TABLE users ADD COLUMN email_verification_token TEXT;
    END IF;
    IF NOT EXISTS (SELECT 1 FROM information_schema.columns WHERE table_name = 'users' AND column_name = 'notify_job_emails') THEN
        ALTER TABLE users ADD COLUMN notify_job_emails BOOLEAN NOT NULL DEFAULT true;
    END IF;
END $$;

-- ============================================================================
-- Password Reset Tokens
-- ============================================================================

CREATE TABLE IF NOT EXISTS password_reset_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_hash
    ON password_reset_tokens(token_hash) WHERE used = false;
CREATE INDEX IF NOT EXISTS idx_password_reset_tokens_user
    ON password_reset_tokens(user_id);
