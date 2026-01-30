# Engram Cloud Architecture

**Date:** 2026-01-30  
**Goal:** Hosted SaaS that monetizes convenience (multi-tenant + ops) while keeping Engram core open source.

## Product Line

- **Engram OSS (MIT):** single-tenant, BYO storage, self-hosted.
- **Engram Cloud (SaaS):** multi-tenant, managed hosting, team workspaces, quotas, metering, dashboard.
- **Engram Enterprise:** Cloud + SSO/SCIM, audit, governance, SLA.

## High-level System

Clients:
- MCP agents (JSON-RPC)
- REST clients (future SDKs)
- Web console (dashboard)

Control Plane (Neon Postgres):
- tenants, members, invites
- API keys, usage events, quotas/plans
- subscriptions (Stripe)

Data Plane (per-tenant):
- SQLite DB per tenant workspace on Fly volume
- Optional object storage (Cloudflare R2): backups + images + exports

Edge:
- Cloudflare (CDN/WAF) -> Gateway

## Request Flow (Auth -> Tenant -> Quota -> Proxy)

1) **Auth**
- JWT (Neon Auth) OR API key
- Validate JWT via JWKS cache (refresh on kid miss)
- Validate API key via prefix lookup + argon2/bcrypt verify

2) **Tenant Resolution**
- Preferred: `{tenant-slug}.engram.cloud` -> slug from host
- Fallback: `api.engram.cloud` requires header `X-Engram-Tenant: {slug}`
- If ambiguous/missing -> 400

3) **Membership & Status**
- If JWT: enforce membership `(tenant_id, user_id)` in `tenant_members`
- If API key: key is tenant-bound, still enforce `tenants.status = active`

4) **Quota & Rate Limit**
- Rate limit: token bucket per tenant + route (Redis or in-memory + sticky routing for MVP)
- Quotas: check plan limits (memories, workspaces, API calls/day, storage bytes, etc.)

5) **Proxy to Tenant Engine**
- Gateway proxies to tenant engine instance
- Engine owns tenant SQLite and implements MCP + REST resources

6) **Usage Metering**
- Record a usage event per request (buffered)
- Flush batches to control plane (idempotent via `request_id`)

## Tenant Isolation Strategy

### MVP: SQLite-per-tenant
- File layout: `/data/tenants/{tenant_id}/engram.db`
- Strong isolation, simple backup/restore, predictable performance

### Runtime Model (explicit)
Gateway needs a deterministic way to route requests.

**Recommended MVP:** shared engine process, tenant DB selected by `tenant_id`  
(You can move to process-per-tenant later.)

- `workspaces.db_path` points to the tenant db file
- Gateway injects `tenant_id` into request context
- Engine opens SQLite connection for that tenant (pool per tenant, capped)

**Future:** process-per-tenant
- Add runtime registry: `machine_id`, `internal_url`, `state`, `heartbeat`

## Storage & Backups

### SQLite
- WAL mode on
- Backups must be consistent:
  - snapshot using SQLite backup API or `VACUUM INTO`
  - upload snapshot to R2 with metadata (`tenant_id`, `schema_version`, `ts`)

### R2
- Backups: `r2://engram-backups/tenants/{tenant_id}/db/{ts}.sqlite`
- Images: `r2://engram-images/tenants/{tenant_id}/images/{memory_id}/{ts}_{idx}_{hash}.{ext}`
- Exports: `r2://engram-exports/tenants/{tenant_id}/{ts}.json`

## Observability (M1 minimum)

Structured log fields:
- request_id, tenant_id, tenant_slug
- user_id (if JWT)
- route, method, status_code
- latency_ms
- error_code (auth/quota/mcp)

Metrics:
- requests_total{route,status}
- latency_ms p50/p95/p99
- auth_failures_total
- rate_limited_total
- quota_exceeded_total
- usage_flush_failures_total

## Security Baseline

- TLS only + HSTS
- Body size limits (2-5MB)
- CORS restricted (dashboard + SDK origins)
- API keys hashed at rest; prefix stored for lookup
- Key rotation + revoke immediately
- Encrypted backups in R2
