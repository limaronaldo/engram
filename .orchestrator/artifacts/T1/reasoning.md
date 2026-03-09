## Task: Refactor tool definitions to structured format with MCP 2025-11-25 annotations

### Approach
Added `ToolAnnotations` (per MCP 2025-11-25 spec) to `protocol.rs` and a `ToolDef` struct to `tools.rs`. Used `const fn` constructors so the array can remain a `const` slice. Converted all 150 tool tuple entries to `ToolDef` struct format via a Python transformation script, then updated `get_tool_definitions()` to pass annotations into `ToolDefinition`. Added 6 unit tests covering serialization, classification, and exhaustive schema validation.

### Files Changed
- `src/mcp/protocol.rs` — Added `ToolAnnotations` struct with camelCase serde rename, `skip_serializing_if = "Option::is_none"` on all fields, and 4 `const fn` constructors (`read_only`, `destructive`, `idempotent`, `mutating`). Extended `ToolDefinition` with `annotations: Option<ToolAnnotations>`.
- `src/mcp/tools.rs` — Added `ToolDef` struct (name, description, schema, annotations). Converted `TOOL_DEFINITIONS` from `&[(&str, &str, &str)]` to `&[ToolDef]` with appropriate annotations on each of the 150 tools. Updated `get_tool_definitions()` to map `ToolDef` fields. Added 6 unit tests.

### Decisions Made
- Chose `const fn` constructors over deriving `Default` in the const context — `..Default::default()` is not usable in `const` expressions pre-stabilization; explicit field initialization works in Rust 1.94.
- `TOOL_DEFINITIONS` stays `const` (no heap allocation) — all fields are `&'static str` and `ToolAnnotations` (only `Option<bool>` fields).
- Annotations are always `Some(...)` in `get_tool_definitions()` — every tool gets a classification; there are no "unknown" tools.
- `mutating()` returns all-`None` fields, which serializes as `{}` — empty object is not emitted because the outer `Option` in `ToolDefinition.annotations` would still be `Some({})`. This is intentional per spec: even for mutating tools, the annotation object is present to signal that hints were considered.
- Tool classification follows the spec guidance: read_only=true means no state modification; destructive=true warns of irreversibility; idempotent=true means repeated calls are safe.

### Verification
- Tests pass: yes (297 existing + 6 new = 303 total, all passing)
- Lint clean: yes (clippy -D warnings, 0 warnings)
- Type check: yes (cargo build clean)
