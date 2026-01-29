# Engram Invariants

These are truths that must **always** hold. They guide implementation, testing, and code review.

## Core Memory Invariants

1. **Memory content is never empty** - Every memory must have non-whitespace content.

2. **Memory IDs are monotonically increasing** - Once assigned, IDs never change or repeat.

3. **Timestamps are RFC3339 UTC** - All timestamps stored as TEXT in RFC3339 format, parsed as `DateTime<Utc>`.

4. **Content hash is deterministic** - Same content always produces same hash (SHA256 of normalized content).

## Workspace Invariants

5. **Workspace names are normalized** - Always lowercase, `[a-z0-9_-]`, max 64 chars, no leading underscore.

6. **"default" workspace always exists** - Every query without explicit workspace uses "default".

7. **Workspace deletion moves or deletes all memories** - No orphaned memories after workspace delete.

## Tier Invariants (Critical)

8. **Permanent tier memories have no expiration** - `tier = 'permanent'` implies `expires_at IS NULL`. Enforced at write-time.

9. **Daily tier memories always have expiration** - `tier = 'daily'` implies `expires_at IS NOT NULL`. Default: created_at + 24h.

10. **Promotion clears expiration** - `promote_to_permanent()` sets `expires_at = NULL`.

## Identity Invariants

11. **Alias normalization is idempotent** - `normalize_alias(normalize_alias(x)) == normalize_alias(x)`

12. **Aliases are globally unique** - One alias cannot map to two different canonical IDs.

13. **Alias conflict is explicit rejection** - If alias exists for different identity, return error (never silently overwrite).

14. **Identity deletion cascades** - Deleting identity removes all aliases and memory links.

## Session/Transcript Invariants

15. **Chunks have bounded size** - Max 10 messages OR 8000 chars per chunk (whichever first).

16. **Chunk overlap preserves context** - Last N messages of chunk N appear as first N messages of chunk N+1.

17. **TranscriptChunk has default 7-day TTL** - Unless explicitly permanent.

## Search Invariants

18. **Search never panics on bad input** - Empty query, invalid regex, malformed filters return empty results.

19. **TranscriptChunks excluded by default** - Regular search excludes `memory_type = 'transcript_chunk'` unless explicit.

## Cache Invariants

20. **Embedding cache has bounded memory** - Max bytes enforced, LRU eviction when exceeded.

21. **Cache get never blocks writers** - Read lock for get, write lock only for put/evict.

## Concurrency Invariants

22. **SQLite connections are not shared across threads** - Each thread/task gets own connection from pool.

23. **Transactions are short-lived** - No network I/O inside transactions.

24. **All external calls have timeouts** - HTTP, embedding APIs, cloud sync.

## Error Invariants

25. **No unwrap() in production paths** - All fallible operations use `?` or explicit error handling.

26. **Errors include context** - Memory ID, workspace name, operation type in error messages.

27. **Validation errors list all problems** - Not just first failure.

## Quota Invariants (engram-cloud)

28. **Quota check happens before mutation** - Check quota, then create memory (not reverse).

29. **Storage-counted metrics query tenant SQLite** - Workspaces, Identities, Sessions counted from storage, not control plane.

30. **Quota exceeded returns structured error** - Includes metric name, current value, max value.

---

## Testing Requirements

For each invariant category:

| Category | Unit Tests | Property Tests | Golden Tests |
|----------|-----------|----------------|--------------|
| Workspace normalization | validate edge cases | idempotency, charset | - |
| Tier invariants | promotion, creation | - | - |
| Alias normalization | edge cases | idempotency | - |
| Search | empty/invalid input | never panics | fixture-based |
| Chunking | boundary conditions | size limits hold | sample conversations |

---

## Checklist for New Code

- [ ] Does it respect all applicable invariants?
- [ ] Are there new invariants to document?
- [ ] No `unwrap()` in non-test code?
- [ ] Errors include context?
- [ ] Bounded memory/concurrency if applicable?
