# Engram Cloud API Reference

**Base URLs**
- Tenant: `https://{tenant-slug}.engram.cloud`
- Global: `https://api.engram.cloud` (requires `X-Engram-Tenant`)

## Authentication

### JWT (Dashboard / users)
Header:
- `Authorization: Bearer <jwt>`

Validation:
- JWKS cache (TTL 10-30 minutes)
- enforce `iss`, `aud=engram-cloud`
- allow 60 seconds clock skew

### API Key (Programmatic / CLI)
Headers (choose one; both is a 400):
- `Authorization: Bearer eng_live_...`
- OR `X-API-Key: eng_live_...`

Key format:
- `eng_live_` / `eng_test_` + 32 chars
- tenant-bound
- scopes stored in control plane (`memories:read`, `memories:write`, `search`, `admin`)

## Tenant Resolution

1) If host matches `{slug}.engram.cloud` -> slug from host
2) Else require `X-Engram-Tenant: {slug}`

If missing -> 400

## Core Endpoints

Health:
- `GET /health`
- `GET /ready`

MCP:
- `POST /v1/mcp` (JSON-RPC 2.0)
- `POST /v1/mcp/batch` (array of JSON-RPC requests)

REST (proxy to engine):
- `POST /v1/memories`
- `GET /v1/memories/:id`
- `PATCH /v1/memories/:id`
- `DELETE /v1/memories/:id`
- `POST /v1/search`
- `GET /v1/usage`

Images (cloud-only, backed by R2):
- `POST /v1/images` (multipart)
- `POST /v1/images/base64` (json)
- `GET /v1/images?memory_id=...`
- `GET /v1/images/:key` (signed URL)
- `DELETE /v1/images/:key`
- `DELETE /v1/images?memory_id=...`

Tenant/Admin (dashboard):
- `POST /tenants`
- `GET /tenants`
- `GET /tenants/:slug`
- `POST /api-keys`
- `GET /api-keys`
- `DELETE /api-keys/:id`
- `POST /invites`
- `POST /invites/accept`

## MCP-over-HTTP (JSON-RPC)

Endpoint:
- `POST /v1/mcp`

Request:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": { "name": "memory_search", "arguments": { "query": "rust async" } }
}
```

### Errors

HTTP status codes:
- 400 invalid request (missing tenant, invalid JSON, both auth headers present)
- 401 unauthorized (bad/missing jwt/api key)
- 403 forbidden (not member / tenant suspended)
- 404 not found (tenant/resource not found)
- 429 too many requests (rate limit)
- 402 payment required (hard plan quota exceeded)
- 500 internal error
- 503 service unavailable (engine/db unavailable)

Quota exceeded (402):
```json
{
  "error": "quota_exceeded",
  "metric": "api_calls_day",
  "current": 1200,
  "limit": 1000,
  "upgrade_url": "https://engram.cloud/upgrade"
}
```

Rate limited (429):
```json
{
  "error": "rate_limited",
  "retry_after": 30
}
```

JSON-RPC error codes:
- Standard: -32700, -32600, -32601, -32602, -32603
- Engram:
  - -32001 unauthorized -> HTTP 401
  - -32002 tenant not found -> HTTP 404
  - -32003 tenant suspended -> HTTP 403
  - -32004 quota exceeded -> HTTP 402
  - -32005 rate limited -> HTTP 429

Tool Registry Rule:
- OSS tools: unchanged (from Engram core)
- Cloud-only tools: prefixed `cloud.*` or tagged `cloudOnly: true` in `tools/list`
