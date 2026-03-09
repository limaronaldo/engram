## Task: T1 — Semantic Structured Compression (RML-1208)

### Approach

Implemented a pure-computation, rule-based semantic compressor in a single new
file. The pipeline is: sentence split -> filler/hedge strip -> entity extraction
-> SVO core extraction -> Jaccard deduplication -> reassembly. No ML, no DB,
no network — just regex + string processing.

Key challenge: the Rust `regex` crate does not support look-around assertions.
Both the proper-noun regex (look-behind `(?<=[a-z])`) and the sentence-split
regex (look-behind `(?<=[.!?])`) had to be rewritten. The sentence splitter
now captures the punctuation character explicitly and re-attaches it to the
preceding fragment. The proper-noun filter uses a sentence-starters HashSet
and a frequency count to approximate "not at sentence start".

### Files Changed

- `src/intelligence/compression_semantic.rs` -- new file implementing
  CompressionConfig, CompressedMemory, SemanticCompressor with all
  specified methods plus 12 unit tests.
- `src/intelligence/mod.rs` -- added `pub mod compression_semantic` declaration
  and re-export of the three public types. Module doc comment updated to mention
  RML-1208.

### Decisions Made

- Used `once_cell::sync::Lazy<Regex>` (already a project dependency) for
  compile-once regex patterns, consistent with `fact_extraction.rs`.
- `estimate_tokens` uses ceiling integer division via `.div_ceil(4)` as
  specified.
- `strip_filler` sorts phrases by descending length so multi-word phrases are
  stripped before their sub-phrases, preventing partial matches.
- `decompress` joins key_facts and appends a parenthetical entity list.
- Pre-existing clippy errors in `consolidation_offline.rs` (3 warnings) were
  left untouched -- they are out of scope for this task.

### Verification

- Tests pass: yes -- 12/12
- Lint clean: yes (no warnings in compression_semantic.rs)
- Type check: yes (cargo build succeeded)
