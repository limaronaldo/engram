# Engram Cloud Architecture

## Overview

This document outlines the architecture for Engram Cloud — the hosted SaaS version of Engram that monetizes convenience while keeping the core engine open source.

## Business Model Summary

```
┌─────────────────────────────────────────────────────────────────┐
│                     Engram Product Line                         │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  Community (MIT)     Cloud (SaaS)        Enterprise (License)   │
│  ───────────────     ────────────        ────────────────────   │
│  • Self-hosted       • Managed hosting   • Self-hosted          │
│  • Single-tenant     • Multi-tenant      • Multi-tenant         │
│  • BYO storage       • Team workspaces   • SSO/SAML/SCIM        │
│  • Free forever      • Usage-based $     • Audit logs           │
│                      • Resource-based $  • Governance           │
│                                          • SLA + Support        │
│                                                                 │
│  Adoption Engine     Revenue Engine      Revenue Engine         │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## Cloud Roadmap (Milestones & Dependencies)

This is a tighter issue list with explicit milestones and dependencies.

### Milestones

**M1 — Gateway & Auth (P1, urgent):** Establish hosted entry point and tenant isolation primitives.  
**M2 — Multi-tenant & Teams (P2):** Enable shared workspaces, metering, and backups.  
**M3 — Billing & Distribution (P2–P3):** Monetization + SDKs + public-facing polish.  
**Core Track — Project Context Discovery (OSS):** Land the user-facing differentiator in the OSS engine.

### Issues (with dependencies)

| Milestone | Issue | Title | Depends On | Est. |
|-----------|-------|-------|------------|------|
| **M1** | RML-904 | API Gateway with JWT Authentication | — | 1w |
| **M1** | RML-919 | Tenant Data Isolation & RLS | RML-904 | 0.5w |
| **M1** | RML-905 | Tenant Provisioning System | RML-904, RML-919 | 1w |
| **M1** | RML-906 | MCP-over-HTTP Protocol Bridge | RML-904 | 1w |
| **M1** | RML-907 | Fly.io Infrastructure Setup | RML-904, RML-905 | 1w |
| **M1** | RML-920 | Observability (Logging, Metrics, Tracing) | RML-904 | 1w |
| **M2** | RML-908 | Team Workspaces & Member Management | RML-905 | 1w |
| **M2** | RML-909 | Usage Tracking & Metering | RML-904, RML-920 | 1w |
| **M2** | RML-910 | Web Dashboard (Next.js + Neon Auth) | RML-905, RML-909 | 2w |
| **M2** | RML-911 | Automated Backup & Restore | RML-905, RML-907 | 1w |
| **M2** | RML-921 | Secrets & API Key Management | RML-905 | 0.5w |
| **M2** | RML-922 | Operational Runbooks | RML-907, RML-911 | 0.5w |
| **M3** | RML-912 | Stripe Billing Integration | RML-909, RML-910 | 1w |
| **M3** | RML-913 | TypeScript/Python SDKs | RML-904, RML-906 | 1.5w |
| **M3** | RML-914 | Marketing Landing Page | M1 complete | 1w |
| **M3** | RML-915 | Documentation Site | M1 complete | 1.5w |
| **M3** | RML-923 | SOC2 Compliance Baseline | M2 complete | 2w |
| **Core** | RML-916 | Project Context Core Module | — | 1w |
| **Core** | RML-917 | Project Context MCP Tools | RML-916 | 0.5w |
| **Core** | RML-918 | Project Context Search Boost | RML-916 | 0.5w |

### Timeline Summary

| Milestone | Issues | Duration | Cumulative |
|-----------|--------|----------|------------|
| **M1** | 6 | ~5 weeks | Week 5 |
| **M2** | 6 | ~6 weeks | Week 11 |
| **M3** | 5 | ~7 weeks | Week 18 |
| **Core** | 3 | ~2 weeks | (parallel) |

> **Note:** Core track runs in parallel with M1-M2. Total to revenue-ready: ~18 weeks.

### Definition of Done

#### M1 — Gateway & Auth
- [ ] Gateway deployed on Fly.io, accepting requests
- [ ] Neon Auth JWT + API key validation working
- [ ] RLS policies enforced on all control plane tables
- [ ] MCP tools callable via HTTP with auth
- [ ] Structured logging with trace IDs
- [ ] Basic metrics dashboard (latency, error rate)
- [ ] First external tenant onboarded (dogfood)

#### M2 — Multi-tenant & Teams
- [ ] Team invites and role-based access working
- [ ] Usage tracking accurate within 1% of actual
- [ ] Dashboard shows usage, workspaces, API keys
- [ ] Daily backups to R2 with verified restore
- [ ] API key rotation without downtime
- [ ] Runbooks documented and tested
- [ ] 10+ beta tenants onboarded

#### M3 — Billing & Distribution
- [ ] Stripe checkout and subscription management live
- [ ] Plan limits enforced (free tier caps)
- [ ] SDKs published to npm and PyPI
- [ ] Landing page live with signup flow
- [ ] Docs site covers all APIs and guides
- [ ] SOC2 readiness assessment complete
- [ ] Public launch announcement ready

## Repository Structure

### Recommended Split

```
github.com/limaronaldo/
├── engram/                    # PUBLIC (MIT) - Core engine
│   ├── src/
│   │   ├── storage/          # SQLite, queries, migrations
│   │   ├── search/           # BM25, hybrid, fuzzy
│   │   ├── embedding/        # TF-IDF, OpenAI
│   │   ├── intelligence/     # Auto-capture, suggestions
│   │   ├── sync/             # BYO S3/R2 sync
│   │   ├── mcp/              # MCP protocol
│   │   └── bin/
│   │       ├── server.rs     # Single-tenant server
│   │       └── cli.rs        # CLI tool
│   └── README.md
│
├── engram-cloud/              # PRIVATE - Cloud control plane
│   ├── gateway/              # Multi-tenant API gateway (Rust/Axum)
│   ├── dashboard/            # Next.js web dashboard
│   │   ├── app/              # App Router pages
│   │   ├── lib/
│   │   │   ├── auth.ts       # Neon Auth client setup
│   │   │   ├── db.ts         # Drizzle + Neon connection
│   │   │   └── stripe.ts     # Stripe client
│   │   └── components/       # React components
│   ├── db/                   # Neon PostgreSQL schema
│   │   ├── schema.ts         # Drizzle schema (tenants, usage, etc.)
│   │   └── migrations/       # Database migrations
│   ├── billing/              # Stripe integration
│   ├── quotas/               # Rate limiting, usage tracking
│   ├── infra/                # Fly.io config, deploy scripts
│   └── workers/              # Background jobs
│
└── engram-enterprise/         # PRIVATE (optional) - Enterprise features
    ├── sso/                  # SAML/SCIM providers
    ├── audit/                # Audit log system
    ├── governance/           # Retention policies
    └── compliance/           # SOC2, HIPAA helpers
```

## Cloud Architecture

### High-Level Design

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              Clients                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐                │
│  │ MCP      │  │ REST     │  │ SDK      │  │ Web      │                │
│  │ Agents   │  │ API      │  │ (future) │  │ Console  │                │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘                │
└───────┼─────────────┼─────────────┼─────────────┼───────────────────────┘
        │             │             │             │
        └─────────────┴──────┬──────┴─────────────┘
                             │
┌────────────────────────────┼────────────────────────────────────────────┐
│                            ▼                                            │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │                      API Gateway (Cloud)                         │   │
│  │  • API key validation                                            │   │
│  │  • Rate limiting (per-tenant)                                    │   │
│  │  • Request routing                                               │   │
│  │  • Usage metering                                                │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                            │                                            │
│              ┌─────────────┼─────────────┐                             │
│              │             │             │                             │
│              ▼             ▼             ▼                             │
│  ┌───────────────┐ ┌───────────────┐ ┌───────────────┐                 │
│  │   Tenant A    │ │   Tenant B    │ │   Tenant C    │                 │
│  │   ─────────   │ │   ─────────   │ │   ─────────   │                 │
│  │ Engram Engine │ │ Engram Engine │ │ Engram Engine │                 │
│  │   (isolated)  │ │   (isolated)  │ │   (isolated)  │                 │
│  └───────┬───────┘ └───────┬───────┘ └───────┬───────┘                 │
│          │                 │                 │                          │
│          ▼                 ▼                 ▼                          │
│  ┌───────────────────────────────────────────────────────────────┐     │
│  │                    Managed Storage                             │     │
│  │  ┌─────────┐  ┌─────────┐  ┌─────────┐                        │     │
│  │  │ SQLite  │  │ SQLite  │  │ SQLite  │   (per-tenant DBs)     │     │
│  │  │ Tenant A│  │ Tenant B│  │ Tenant C│   on Fly.io volumes    │     │
│  │  └─────────┘  └─────────┘  └─────────┘                        │     │
│  │                                                                │     │
│  │  ┌─────────────────────────────────────────────────────┐      │     │
│  │  │              Object Storage (Cloudflare R2)          │      │     │
│  │  │  • DB backups  • Large embeddings  • Exports         │      │     │
│  │  └─────────────────────────────────────────────────────┘      │     │
│  └───────────────────────────────────────────────────────────────┘     │
│                                                                         │
│  ┌───────────────────────────────────────────────────────────────┐     │
│  │                 Neon PostgreSQL (Control Plane)                │     │
│  │  ┌─────────────────────┐  ┌─────────────────────────────┐     │     │
│  │  │    neon_auth.*      │  │      engram schema          │     │     │
│  │  │  • users            │  │  • tenants                  │     │     │
│  │  │  • sessions         │  │  • workspaces               │     │     │
│  │  │  • accounts         │  │  • api_keys                 │     │     │
│  │  │  • organizations    │  │  • usage_daily              │     │     │
│  │  │  • jwks             │  │  • subscriptions            │     │     │
│  │  └─────────────────────┘  └─────────────────────────────┘     │     │
│  └───────────────────────────────────────────────────────────────┘     │
│                                                                         │
│                         ENGRAM CLOUD                                    │
└─────────────────────────────────────────────────────────────────────────┘
```

### Tenant Isolation Strategy

**Option A: Process-per-tenant (Recommended for MVP)**
- Each tenant gets their own Engram server process
- SQLite DB file per tenant
- Simple, strong isolation
- Scale: ~1000 tenants per node

**Option B: Shared process, isolated DBs (Scale)**
- Single gateway routes to tenant DBs
- Connection pooling across tenants
- More efficient at scale
- Requires careful isolation

### Key Components

#### 1. API Gateway (`engram-cloud/gateway/`)

```rust
// Handles multi-tenant routing
pub struct CloudGateway {
    auth: AuthService,
    tenants: TenantRegistry,
    quotas: QuotaService,
    metrics: MetricsCollector,
}

impl CloudGateway {
    async fn handle_request(&self, req: Request) -> Response {
        // 1. Validate API key
        let api_key = self.auth.validate_key(&req)?;
        
        // 2. Get tenant
        let tenant = self.tenants.get(api_key.tenant_id)?;
        
        // 3. Check quotas
        self.quotas.check(&tenant, &req)?;
        
        // 4. Route to tenant's Engram instance
        let response = tenant.engram.handle(req).await?;
        
        // 5. Record usage
        self.metrics.record(&tenant, &req, &response);
        
        response
    }
}
```

#### 2. Tenant Management (`engram-cloud/tenants/`)

```rust
pub struct Tenant {
    pub id: TenantId,
    pub name: String,
    pub plan: Plan,
    pub members: Vec<Member>,
    pub api_keys: Vec<ApiKey>,
    pub db_path: PathBuf,
    pub created_at: DateTime<Utc>,
}

pub enum Plan {
    Free { memory_limit: usize },
    Pro { memory_limit: usize, searches_per_month: usize },
    Team { members: usize, memory_limit: usize },
    Enterprise,
}
```

#### 3. Billing Integration (`engram-cloud/billing/`)

```rust
// Stripe integration for usage-based billing
pub struct BillingService {
    stripe: StripeClient,
}

impl BillingService {
    async fn record_usage(&self, tenant: &Tenant, usage: Usage) {
        // Report to Stripe metered billing
        self.stripe.create_usage_record(
            tenant.stripe_subscription_id,
            UsageRecord {
                quantity: usage.memories_created,
                timestamp: Utc::now(),
            }
        ).await;
    }
}

pub struct Usage {
    pub memories_created: u64,
    pub memories_stored: u64,
    pub searches: u64,
    pub api_calls: u64,
    pub storage_bytes: u64,
}
```

#### 4. Quota & Rate Limiting (`engram-cloud/quotas/`)

```rust
pub struct QuotaService {
    redis: RedisClient,
}

impl QuotaService {
    async fn check(&self, tenant: &Tenant, req: &Request) -> Result<()> {
        let key = format!("quota:{}:{}", tenant.id, req.endpoint);
        
        // Rate limit per minute
        let count = self.redis.incr(&key).await?;
        if count > tenant.plan.rate_limit() {
            return Err(QuotaExceeded::RateLimit);
        }
        
        // Check storage quota
        if tenant.storage_used > tenant.plan.storage_limit() {
            return Err(QuotaExceeded::Storage);
        }
        
        Ok(())
    }
}
```

## Cloud MVP Features

### Phase 1: Core Cloud (4-6 weeks)

- [ ] Multi-tenant gateway with API key auth
- [ ] Tenant provisioning (create workspace, get API key)
- [ ] Per-tenant SQLite isolation
- [ ] Basic rate limiting (requests/minute)
- [ ] Simple usage tracking
- [ ] Stripe integration for billing
- [ ] Landing page + signup flow

### Phase 2: Team Features (4 weeks)

- [ ] Team workspaces (invite members)
- [ ] Shared memories across team
- [ ] Role-based access (admin, member, viewer)
- [ ] API key management (create, revoke, rotate)
- [ ] Usage dashboard

### Phase 3: Production Hardening (4 weeks)

- [ ] Automated backups to S3/R2
- [ ] Point-in-time recovery
- [ ] Monitoring + alerting
- [ ] SOC2 compliance prep
- [ ] DDoS protection
- [ ] Geographic redundancy

## Pricing Model

### Usage-Based (Meilisearch-style)

| Metric | Free | Pro | Team |
|--------|------|-----|------|
| Memories | 1,000 | 50,000 | 500,000 |
| Searches/mo | 10,000 | 100,000 | Unlimited |
| API calls/mo | 50,000 | 500,000 | Unlimited |
| Storage | 100 MB | 5 GB | 50 GB |
| Team members | 1 | 1 | 10 |
| Price | $0 | $29/mo | $99/mo |

### Resource-Based (Dedicated)

For high-performance needs:
- Dedicated compute
- Guaranteed latency SLA
- Custom pricing

## Tech Stack Recommendations

### Gateway
- **Rust + Axum** (consistency with core)
- Or **Go** (fast iteration, good for APIs)

### Database & Auth
- **Neon PostgreSQL** for control plane metadata (tenants, usage, billing)
- **Neon Auth with Better Auth** for authentication
  - Users, sessions, organizations stored in `neon_auth` schema
  - Branch-aware auth (preview envs get isolated auth state)
  - JWT tokens for API auth + RLS policies
  - SDK: `@neondatabase/neon-js/auth`

### Billing
- **Stripe** (metered billing, subscriptions)

### Infrastructure
- **Fly.io** (simple, scales well, SQLite-friendly for tenant DBs)
- **Neon PostgreSQL** (control plane, serverless, branching)
- **Cloudflare R2** for storage (S3-compatible, cheap)

### Monitoring
- **Axiom** or **Datadog** for logs/metrics
- **Sentry** for errors
- **BetterUptime** for status page

## Neon Auth Integration

### Overview

Engram Cloud uses **Neon Auth with Better Auth** for authentication. All auth data lives directly in the Neon PostgreSQL database under the `neon_auth` schema, making it branch-compatible for preview environments.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      Neon PostgreSQL                            │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  neon_auth schema (managed)      engram schema (application)    │
│  ─────────────────────────       ──────────────────────────     │
│  • neon_auth.user               • tenants                       │
│  • neon_auth.session            • workspaces                    │
│  • neon_auth.account            • api_keys                      │
│  • neon_auth.organization       • usage_daily                   │
│  • neon_auth.jwks               • subscriptions                 │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Dashboard Auth Setup (`dashboard/lib/auth.ts`)

```typescript
import { createAuthClient } from '@neondatabase/neon-js/auth';

export const authClient = createAuthClient(
  process.env.NEXT_PUBLIC_NEON_AUTH_URL!
);

// Get current user session
export async function getSession() {
  return authClient.getSession();
}

// Sign out
export async function signOut() {
  return authClient.signOut();
}
```

### Auth UI Components (`dashboard/app/layout.tsx`)

```tsx
import { NeonAuthUIProvider, AuthView } from '@neondatabase/neon-js/auth/react/ui';
import { authClient } from '@/lib/auth';

export default function RootLayout({ children }) {
  return (
    <NeonAuthUIProvider authClient={authClient}>
      {children}
    </NeonAuthUIProvider>
  );
}

// Sign-in page
export function SignInPage() {
  return <AuthView pathname="sign-in" />;
}
```

### Database Schema with RLS (`db/schema.ts`)

```typescript
import { pgTable, uuid, text, timestamp, bigint, boolean } from 'drizzle-orm/pg-core';
import { sql } from 'drizzle-orm';

// Tenants table with RLS
export const tenants = pgTable('tenants', {
  id: uuid('id').primaryKey().defaultRandom(),
  name: text('name').notNull(),
  ownerId: text('owner_id').notNull(),  // References neon_auth.user.id
  plan: text('plan').default('free'),
  stripeCustomerId: text('stripe_customer_id'),
  stripeSubscriptionId: text('stripe_subscription_id'),
  createdAt: timestamp('created_at').defaultNow(),
});

// Workspaces within a tenant
export const workspaces = pgTable('workspaces', {
  id: uuid('id').primaryKey().defaultRandom(),
  tenantId: uuid('tenant_id').references(() => tenants.id),
  name: text('name').notNull(),
  dbPath: text('db_path').notNull(),  // Path to tenant's SQLite DB
  createdAt: timestamp('created_at').defaultNow(),
});

// API keys for programmatic access
export const apiKeys = pgTable('api_keys', {
  id: uuid('id').primaryKey().defaultRandom(),
  tenantId: uuid('tenant_id').references(() => tenants.id),
  name: text('name').notNull(),
  keyHash: text('key_hash').notNull(),  // bcrypt hash of key
  prefix: text('prefix').notNull(),      // ek_live_xxx (shown to user)
  lastUsedAt: timestamp('last_used_at'),
  expiresAt: timestamp('expires_at'),
  createdAt: timestamp('created_at').defaultNow(),
});

// Daily usage tracking
export const usageDaily = pgTable('usage_daily', {
  tenantId: uuid('tenant_id').references(() => tenants.id),
  workspaceId: uuid('workspace_id').references(() => workspaces.id),
  date: timestamp('date'),
  apiCalls: bigint('api_calls', { mode: 'number' }).default(0),
  memoriesCreated: bigint('memories_created', { mode: 'number' }).default(0),
  searchQueries: bigint('search_queries', { mode: 'number' }).default(0),
  storageBytes: bigint('storage_bytes', { mode: 'number' }).default(0),
});
```

### RLS Policies (SQL)

```sql
-- Enable RLS on all tables
ALTER TABLE tenants ENABLE ROW LEVEL SECURITY;
ALTER TABLE workspaces ENABLE ROW LEVEL SECURITY;
ALTER TABLE api_keys ENABLE ROW LEVEL SECURITY;

-- Helper function to get current user ID from Neon Auth JWT
CREATE OR REPLACE FUNCTION auth.uid() RETURNS TEXT AS $$
  SELECT current_setting('request.jwt.claims', true)::json->>'sub'
$$ LANGUAGE SQL STABLE;

-- Tenants: users can only see their own tenants
CREATE POLICY "Users can view own tenants"
  ON tenants FOR SELECT
  TO authenticated
  USING (owner_id = auth.uid());

CREATE POLICY "Users can create tenants"
  ON tenants FOR INSERT
  TO authenticated
  WITH CHECK (owner_id = auth.uid());

-- Workspaces: users can see workspaces in their tenants
CREATE POLICY "Users can view tenant workspaces"
  ON workspaces FOR SELECT
  TO authenticated
  USING (tenant_id IN (
    SELECT id FROM tenants WHERE owner_id = auth.uid()
  ));

-- API keys: users can manage keys for their tenants
CREATE POLICY "Users can manage tenant API keys"
  ON api_keys FOR ALL
  TO authenticated
  USING (tenant_id IN (
    SELECT id FROM tenants WHERE owner_id = auth.uid()
  ));
```

### Gateway JWT Validation (`gateway/src/auth.rs`)

```rust
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct NeonAuthClaims {
    pub sub: String,        // User ID
    pub email: String,
    pub role: String,
    pub exp: usize,
}

pub struct AuthService {
    jwks: JwksCache,  // Cached JWKS from Neon
}

impl AuthService {
    /// Validate JWT from Neon Auth
    pub async fn validate_jwt(&self, token: &str) -> Result<NeonAuthClaims, AuthError> {
        let header = jsonwebtoken::decode_header(token)?;
        let kid = header.kid.ok_or(AuthError::MissingKeyId)?;
        
        let key = self.jwks.get_key(&kid).await?;
        let validation = Validation::new(Algorithm::RS256);
        
        let token_data = decode::<NeonAuthClaims>(
            token,
            &DecodingKey::from_rsa_pem(key.as_bytes())?,
            &validation
        )?;
        
        Ok(token_data.claims)
    }
    
    /// Validate API key (for programmatic access)
    pub async fn validate_api_key(&self, key: &str) -> Result<TenantId, AuthError> {
        // API keys start with ek_live_ or ek_test_
        let prefix = &key[..12];
        
        // Look up in database by prefix, verify hash
        let api_key = sqlx::query_as!(ApiKey,
            "SELECT * FROM api_keys WHERE prefix = $1",
            prefix
        ).fetch_optional(&self.pool).await?;
        
        match api_key {
            Some(k) if bcrypt::verify(key, &k.key_hash)? => {
                Ok(k.tenant_id)
            }
            _ => Err(AuthError::InvalidApiKey)
        }
    }
}
```

### Environment Variables

```bash
# Neon Database
DATABASE_URL=postgresql://user:pass@ep-xxx.us-east-2.aws.neon.tech/engram_cloud

# Neon Auth (provided by Neon console)
NEXT_PUBLIC_NEON_AUTH_URL=https://auth.neon.tech/your-project-id

# Stripe
STRIPE_SECRET_KEY=sk_live_xxx
STRIPE_WEBHOOK_SECRET=whsec_xxx

# Gateway
GATEWAY_PORT=8080
JWKS_URL=https://auth.neon.tech/your-project-id/.well-known/jwks.json
```

### Benefits of Neon Auth

1. **Branch-aware**: Preview branches get isolated auth state
2. **SQL-queryable**: Join `neon_auth.user` with your tables directly
3. **No external deps**: Auth data lives in your database
4. **RLS integration**: JWT claims work with Row Level Security
5. **Better Auth foundation**: Familiar APIs, extensible

## API Design

### Authentication

```bash
# Option 1: JWT from Neon Auth (web dashboard)
curl -H "Authorization: Bearer eyJhbG..." \
  https://api.engram.dev/v1/memories

# Option 2: API key (programmatic access)
curl -H "Authorization: Bearer ek_live_xxx" \
  https://api.engram.dev/v1/memories
```

### Endpoints

```
POST   /v1/memories              # Create memory
GET    /v1/memories/:id          # Get memory
PATCH  /v1/memories/:id          # Update memory
DELETE /v1/memories/:id          # Delete memory
GET    /v1/memories              # List memories
POST   /v1/search                # Search memories
GET    /v1/stats                 # Usage stats
POST   /v1/mcp                   # MCP protocol endpoint
```

### MCP over HTTP

```bash
# MCP calls wrapped in HTTP
curl -X POST https://api.engram.dev/v1/mcp \
  -H "Authorization: Bearer ek_live_xxx" \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "method": "tools/call",
    "params": {
      "name": "memory_search",
      "arguments": {"query": "rust async"}
    }
  }'
```

## Migration Path

### Community → Cloud

```bash
# Export from self-hosted
engram-cli export --output memories.json

# Import to Cloud
curl -X POST https://api.engram.dev/v1/import \
  -H "Authorization: Bearer ek_live_xxx" \
  -F "file=@memories.json"
```

### Cloud → Self-hosted

```bash
# Export from Cloud (data portability)
curl https://api.engram.dev/v1/export \
  -H "Authorization: Bearer ek_live_xxx" \
  -o memories.json

# Import to self-hosted
engram-cli import --input memories.json
```

## Security Considerations

1. **Tenant Isolation**: Separate SQLite files, no cross-tenant queries
2. **API Key Security**: Hash keys at rest, support rotation
3. **Encryption**: TLS everywhere, encrypt backups
4. **Rate Limiting**: Prevent abuse, DDoS protection
5. **Audit Logging**: Track all access (Enterprise)
6. **Data Residency**: Support region selection (future)

## Next Steps

1. **Create `engram-cloud` private repo**
2. **Set up basic gateway with API key auth**
3. **Implement tenant provisioning**
4. **Deploy MVP to Fly.io**
5. **Set up Stripe billing**
6. **Build landing page with waitlist**

---

*Last updated: January 2026*
