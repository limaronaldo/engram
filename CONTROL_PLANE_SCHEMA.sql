-- Engram Cloud Control Plane (Neon Postgres)
-- Provides: tenant registry, membership, API keys, plans/quotas, usage events, billing hooks.

BEGIN;

-- UUID generation
CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- Tenants
CREATE TABLE IF NOT EXISTS tenants (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  slug text NOT NULL UNIQUE,
  name text NOT NULL,
  status text NOT NULL DEFAULT 'active', -- active | suspended | deleted
  owner_user_id text NOT NULL,           -- neon_auth user id (sub)
  plan text NOT NULL DEFAULT 'free',     -- free | pro | team | enterprise
  created_at timestamptz NOT NULL DEFAULT now(),
  updated_at timestamptz NOT NULL DEFAULT now()
);

-- Members (needed for real team access)
CREATE TABLE IF NOT EXISTS tenant_members (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  user_id text NOT NULL,                 -- neon_auth user id
  role text NOT NULL DEFAULT 'member',   -- owner | admin | member | viewer
  created_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_tenant_members_user
  ON tenant_members (user_id);

-- Invites (email-based)
CREATE TABLE IF NOT EXISTS tenant_invites (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  email text NOT NULL,
  role text NOT NULL DEFAULT 'member',
  token_hash text NOT NULL,
  expires_at timestamptz NOT NULL,
  accepted_at timestamptz,
  invited_by text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, email)
);

-- Workspaces (if you support multiple dbs per tenant)
CREATE TABLE IF NOT EXISTS workspaces (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  name text NOT NULL,
  db_path text NOT NULL,                 -- e.g. /data/tenants/{tenant_id}/engram.db
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, name)
);

-- API keys (tenant-bound)
CREATE TABLE IF NOT EXISTS api_keys (
  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  name text NOT NULL,
  prefix text NOT NULL,                  -- visible prefix for lookup (e.g. eng_live_ab12)
  key_hash text NOT NULL,                -- bcrypt/argon2 hash of full key
  scopes text[] NOT NULL DEFAULT ARRAY['admin']::text[],
  last_used_at timestamptz,
  expires_at timestamptz,
  revoked_at timestamptz,
  created_by text NOT NULL,
  created_at timestamptz NOT NULL DEFAULT now(),
  UNIQUE (tenant_id, prefix)
);

CREATE INDEX IF NOT EXISTS idx_api_keys_tenant
  ON api_keys (tenant_id);

-- Plans / Quotas
CREATE TABLE IF NOT EXISTS plan_quotas (
  plan text PRIMARY KEY,
  memories_limit bigint,
  workspaces_limit bigint,
  identities_limit bigint,
  sessions_limit bigint,
  api_calls_day_limit bigint,
  storage_bytes_limit bigint,
  rpm_limit int,
  burst_limit int,
  updated_at timestamptz NOT NULL DEFAULT now()
);

-- Current usage rollups (fast reads)
CREATE TABLE IF NOT EXISTS usage_daily (
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  day date NOT NULL,
  api_calls bigint NOT NULL DEFAULT 0,
  search_queries bigint NOT NULL DEFAULT 0,
  memories_created bigint NOT NULL DEFAULT 0,
  storage_bytes bigint NOT NULL DEFAULT 0,
  PRIMARY KEY (tenant_id, day)
);

-- Raw usage events (idempotent ingestion)
CREATE TABLE IF NOT EXISTS usage_events (
  request_id uuid PRIMARY KEY,           -- idempotency key
  tenant_id uuid NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
  workspace_id uuid REFERENCES workspaces(id) ON DELETE SET NULL,
  route text NOT NULL,
  status_code int NOT NULL,
  api_calls int NOT NULL DEFAULT 1,
  search_queries int NOT NULL DEFAULT 0,
  memories_created int NOT NULL DEFAULT 0,
  storage_bytes_delta bigint NOT NULL DEFAULT 0,
  occurred_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_usage_events_tenant_time
  ON usage_events (tenant_id, occurred_at DESC);

-- Billing hooks (Stripe)
CREATE TABLE IF NOT EXISTS subscriptions (
  tenant_id uuid PRIMARY KEY REFERENCES tenants(id) ON DELETE CASCADE,
  stripe_customer_id text,
  stripe_subscription_id text,
  status text DEFAULT 'inactive',        -- inactive | active | past_due | canceled
  current_period_end timestamptz,
  updated_at timestamptz NOT NULL DEFAULT now()
);

-- Simple audit log (early SOC2 baseline)
CREATE TABLE IF NOT EXISTS audit_log (
  id bigserial PRIMARY KEY,
  tenant_id uuid REFERENCES tenants(id) ON DELETE CASCADE,
  actor_user_id text,
  action text NOT NULL,                  -- e.g. api_key.created, tenant.invite.sent
  target_type text,
  target_id text,
  metadata jsonb,
  created_at timestamptz NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_audit_tenant_time
  ON audit_log (tenant_id, created_at DESC);

COMMIT;
