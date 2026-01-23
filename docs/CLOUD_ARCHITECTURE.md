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
│   ├── gateway/              # Multi-tenant API gateway
│   ├── auth/                 # Auth0/Clerk integration
│   ├── billing/              # Stripe integration
│   ├── tenants/              # Workspace/org management
│   ├── quotas/               # Rate limiting, usage tracking
│   ├── admin/                # Admin dashboard
│   ├── infra/                # Terraform, K8s manifests
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
│  │  │ Tenant A│  │ Tenant B│  │ Tenant C│                        │     │
│  │  └─────────┘  └─────────┘  └─────────┘                        │     │
│  │                                                                │     │
│  │  ┌─────────────────────────────────────────────────────┐      │     │
│  │  │              Object Storage (S3/R2)                  │      │     │
│  │  │  • DB backups  • Large embeddings  • Exports         │      │     │
│  │  └─────────────────────────────────────────────────────┘      │     │
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

### Auth
- **Clerk** or **Auth0** (faster to market)
- Or build with JWT + refresh tokens

### Billing
- **Stripe** (metered billing, subscriptions)

### Infrastructure
- **Fly.io** (simple, scales well, SQLite-friendly)
- Or **Railway** / **Render** for MVP
- **Cloudflare R2** for storage (S3-compatible, cheap)

### Monitoring
- **Axiom** or **Datadog** for logs/metrics
- **Sentry** for errors
- **BetterUptime** for status page

## API Design

### Authentication

```bash
# All requests require API key
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
