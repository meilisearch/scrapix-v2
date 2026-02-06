-- Scrapix Console Database Schema
-- Run this in your Supabase SQL editor

-- Enable UUID extension if not already enabled
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- profiles: extends auth.users
CREATE TABLE IF NOT EXISTS profiles (
  id UUID PRIMARY KEY REFERENCES auth.users(id) ON DELETE CASCADE,
  email TEXT NOT NULL,
  full_name TEXT,
  created_at TIMESTAMPTZ DEFAULT NOW()
);

-- accounts: billing entities
CREATE TABLE IF NOT EXISTS accounts (
  id TEXT PRIMARY KEY DEFAULT 'acct_' || substr(md5(random()::text), 1, 12),
  name TEXT NOT NULL,
  tier TEXT DEFAULT 'free' CHECK (tier IN ('free', 'starter', 'pro', 'enterprise')),
  active BOOLEAN DEFAULT TRUE,
  stripe_customer_id TEXT,
  created_at TIMESTAMPTZ DEFAULT NOW()
);

-- account_members: links users to accounts
CREATE TABLE IF NOT EXISTS account_members (
  id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
  user_id UUID REFERENCES profiles(id) ON DELETE CASCADE,
  role TEXT DEFAULT 'owner',
  created_at TIMESTAMPTZ DEFAULT NOW(),
  UNIQUE(account_id, user_id)
);

-- api_keys: authentication tokens
CREATE TABLE IF NOT EXISTS api_keys (
  id TEXT PRIMARY KEY DEFAULT 'key_' || substr(md5(random()::text), 1, 12),
  account_id TEXT REFERENCES accounts(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  prefix TEXT NOT NULL,
  key_hash TEXT NOT NULL,
  active BOOLEAN DEFAULT TRUE,
  last_used_at TIMESTAMPTZ,
  created_at TIMESTAMPTZ DEFAULT NOW()
);

-- Index for fast key lookup
CREATE INDEX IF NOT EXISTS idx_api_keys_hash ON api_keys(key_hash);
CREATE INDEX IF NOT EXISTS idx_api_keys_account ON api_keys(account_id);
CREATE INDEX IF NOT EXISTS idx_account_members_user ON account_members(user_id);

-- Enable RLS on all tables
ALTER TABLE profiles ENABLE ROW LEVEL SECURITY;
ALTER TABLE accounts ENABLE ROW LEVEL SECURITY;
ALTER TABLE account_members ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;

-- RLS Policies for profiles
CREATE POLICY "Users can view own profile" ON profiles
  FOR SELECT USING (auth.uid() = id);

CREATE POLICY "Users can update own profile" ON profiles
  FOR UPDATE USING (auth.uid() = id);

-- RLS Policies for accounts (via membership)
CREATE POLICY "Users can view accounts they belong to" ON accounts
  FOR SELECT USING (
    EXISTS (
      SELECT 1 FROM account_members
      WHERE account_members.account_id = accounts.id
      AND account_members.user_id = auth.uid()
    )
  );

CREATE POLICY "Users can update accounts they own" ON accounts
  FOR UPDATE USING (
    EXISTS (
      SELECT 1 FROM account_members
      WHERE account_members.account_id = accounts.id
      AND account_members.user_id = auth.uid()
      AND account_members.role = 'owner'
    )
  );

-- RLS Policies for account_members
CREATE POLICY "Users can view their memberships" ON account_members
  FOR SELECT USING (user_id = auth.uid());

-- RLS Policies for api_keys
CREATE POLICY "Users can view API keys for their accounts" ON api_keys
  FOR SELECT USING (
    EXISTS (
      SELECT 1 FROM account_members
      WHERE account_members.account_id = api_keys.account_id
      AND account_members.user_id = auth.uid()
    )
  );

CREATE POLICY "Users can insert API keys for their accounts" ON api_keys
  FOR INSERT WITH CHECK (
    EXISTS (
      SELECT 1 FROM account_members
      WHERE account_members.account_id = api_keys.account_id
      AND account_members.user_id = auth.uid()
    )
  );

CREATE POLICY "Users can update API keys for their accounts" ON api_keys
  FOR UPDATE USING (
    EXISTS (
      SELECT 1 FROM account_members
      WHERE account_members.account_id = api_keys.account_id
      AND account_members.user_id = auth.uid()
    )
  );

-- Trigger: create profile + account on signup
CREATE OR REPLACE FUNCTION handle_new_user()
RETURNS TRIGGER AS $$
DECLARE
  new_account_id TEXT;
BEGIN
  -- Create profile
  INSERT INTO profiles (id, email, full_name)
  VALUES (NEW.id, NEW.email, NEW.raw_user_meta_data->>'full_name');

  -- Create default account
  INSERT INTO accounts (name)
  VALUES (COALESCE(NEW.raw_user_meta_data->>'full_name', split_part(NEW.email, '@', 1)))
  RETURNING id INTO new_account_id;

  -- Link user to account as owner
  INSERT INTO account_members (account_id, user_id, role)
  VALUES (new_account_id, NEW.id, 'owner');

  RETURN NEW;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER;

-- Drop trigger if exists and recreate
DROP TRIGGER IF EXISTS on_auth_user_created ON auth.users;
CREATE TRIGGER on_auth_user_created
  AFTER INSERT ON auth.users
  FOR EACH ROW EXECUTE FUNCTION handle_new_user();

-- Function for Rust API to validate keys (called via database connection)
CREATE OR REPLACE FUNCTION validate_api_key(key_hash_input TEXT)
RETURNS TABLE (account_id TEXT, tier TEXT, active BOOLEAN) AS $$
BEGIN
  -- Update last_used_at timestamp
  UPDATE api_keys SET last_used_at = NOW()
  WHERE key_hash = key_hash_input AND api_keys.active = true;

  -- Return account info
  RETURN QUERY
  SELECT a.id, a.tier, a.active
  FROM api_keys k
  JOIN accounts a ON k.account_id = a.id
  WHERE k.key_hash = key_hash_input
    AND k.active = true
    AND a.active = true;
END;
$$ LANGUAGE plpgsql SECURITY DEFINER;

-- Grant execute permission to authenticated users and service role
GRANT EXECUTE ON FUNCTION validate_api_key(TEXT) TO authenticated;
GRANT EXECUTE ON FUNCTION validate_api_key(TEXT) TO service_role;
