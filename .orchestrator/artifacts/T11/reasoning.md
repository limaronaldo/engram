## Task: T11 — Proactive Memory Acquisition (RML-1221)

### Approach

Created a single new file `src/intelligence/proactive.rs` containing two public structs:
- `GapDetector` — analyses coverage, detects knowledge gaps, suggests acquisitions
- `InterestTracker` — records user queries and surfaces frequent topics

All DB operations use `rusqlite::Connection` directly (no pool abstraction), matching the pattern in `fact_extraction.rs` and `emotional.rs`. No external crates beyond what is already in the project.

Added `pub mod proactive;` to `src/intelligence/mod.rs` (one-line change, unavoidable for the module to be compiled and tested).

### Files Changed

- `src/intelligence/proactive.rs` — new file: all types, GapDetector, InterestTracker, CREATE_QUERY_LOG_TABLE const, 10 tests
- `src/intelligence/mod.rs` — added `pub mod proactive;` declaration (minimal, required for compilation)

### Decisions Made

- Chose not to add re-exports to `mod.rs` (beyond the `pub mod` declaration) to stay minimal; the task did not require public re-exports.
- `GapDetector::suggest_acquisitions` sorts by priority after collecting all suggestions, ensuring stable ordering across priority classes.
- Temporal gap parsing uses `chrono::DateTime::parse_from_rfc3339` with graceful fallback (skips unparseable pairs) — consistent with the codebase's RFC3339 UTC invariant.
- Stop-word list in `InterestTracker::get_frequent_topics` is kept short but covers the most common English function words; matches the pattern in `suggestions.rs`.
- `limit = 0` means "unlimited" in both `suggest_acquisitions` and `get_frequent_topics`, consistent with `list_facts` in `fact_extraction.rs`.

### Verification

- Tests pass: yes (10/10)
- Lint clean: yes (no errors in proactive.rs; pre-existing errors in gardening.rs are unrelated)
- Format check: yes (`rustfmt --check` passes)
