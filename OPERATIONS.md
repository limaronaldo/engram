# Engram Cloud Operations

## SLOs (M1 baseline)
- Gateway availability: 99.9%
- p95 latency (gateway + storage, excluding model calls): < 250ms
- Error rate: < 0.5%
- Auth failure rate: < 2%

## Alerts (minimum)
- 5xx_rate > 1% for 5m
- p95_latency > 500ms for 10m
- auth_failures spike (3x baseline)
- usage_flush_failures > 0 for 5m
- db_open_failures > 0 for 1m
- rate_limited_total anomaly (abuse)

## Backup & Restore

### Backup (recommended)
1) Create consistent snapshot:
   - SQLite backup API OR `VACUUM INTO '/tmp/{tenant_id}_{ts}.sqlite'`
2) Upload to R2:
   - `tenants/{tenant_id}/db/{ts}_v{schema_version}.sqlite`
3) Store backup metadata row in control plane:
   - tenant_id, ts, schema_version, size_bytes, sha256

### Restore
1) Download snapshot to `/data/tenants/{tenant_id}/restore.sqlite`
2) Stop writes (maintenance lock for tenant)
3) Replace db file atomically:
   - move current -> `.bak`
   - move restore -> active
4) Run integrity check + schema version verify
5) Release lock

### Restore Tests
- weekly: restore a random tenant into a staging sandbox and run smoke tests

## Usage Metering Pipeline

### Why
Writing to Postgres on every request throttles the gateway.

### Design
- Generate `request_id` at gateway edge
- Produce usage event `{request_id, tenant_id, route, status, counters, ts}`
- Buffer in memory
- Flush batches every N seconds or M events
- Control plane insert is idempotent (unique(request_id))

### Counters
- api_calls = 1 per request
- search_queries = 1 per /v1/search or MCP memory_search
- memories_created for create endpoints
- storage_bytes delta (optional in M1)

## Incident Playbook (fast actions)

### Auth outage
- check JWKS refresh
- force JWKS refresh on kid miss
- verify issuer/audience config

### Tenant DB unavailable
- check Fly volume health
- verify path permissions
- restart engine process
- restore from last good backup if corruption

### High latency
- inspect top routes p95
- reduce burst / tighten rate limit
- cap per-tenant SQLite pool
- shed load on expensive routes

## Environments

### Dev
- local gateway + local sqlite
- Neon branch for control plane dev
- relaxed CORS for localhost

### Staging
- separate Neon branch
- real rate limits (soft)
- usage metering can be dry-run

### Prod
- Neon main branch with backups
- strict CORS, TLS/HSTS
- quotas enforced
- alerts + dashboards live
