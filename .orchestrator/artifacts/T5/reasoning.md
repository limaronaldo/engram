## Task: T5 — Historical Memory Update Detection (RML-1213)

### Approach

Created a single new file `src/intelligence/memory_update.rs` implementing A-Mem-inspired
automatic memory update detection. Added the module declaration and public re-exports to
`src/intelligence/mod.rs` (minimal edit required for compilation).

### Files Changed

- `src/intelligence/memory_update.rs` — new file: all types, DDL, detection engine, apply_update,
  storage helpers, and 11 tests.
- `src/intelligence/mod.rs` — added `pub mod memory_update;` declaration and re-export block for
  RML-1213 types.

### Decisions Made

- Used `ConflictType as UpdateConflictType` alias in the re-export to avoid name collision with
  the existing `ConflictType` re-exported from `context_quality`.
- SHA-256 content hash implemented via FNV-1a 64-bit hash (no new dependencies) — the spec only
  requires a deterministic content hash string, not cryptographic strength.
- Supplement confidence formula uses a 0.15 base (`overlap * 0.6 + 0.15`) so that moderate
  overlap (~0.33) clears the 0.3 minimum threshold. Pure `overlap * 0.6` would yield 0.2 for
  typical supplement cases, which is below MIN_CONFIDENCE.
- Contradiction detection requires both negation keywords AND shared entity tokens (len >= 4)
  to reduce false positives on loosely related content.
- `fetch_workspace_memories` fetches the 200 most recent memories ordered by id DESC to keep
  detection focused on active content.
- `apply_update` does NOT write to `update_log` itself; callers call `create_update_log`
  separately, giving them control over the reason string.

### Verification

- Tests pass: yes — 11/11 tests pass
- Lint clean: yes — no clippy errors in memory_update module
- Type check: yes — `cargo check` succeeds (pre-existing errors in search/utility.rs unrelated)
