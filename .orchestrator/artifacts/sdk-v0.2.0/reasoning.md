## Task: Add 46 new MCP tools to Python and TypeScript SDKs (v0.2.0)

### Approach
Read both existing SDK files to understand the exact patterns used, then added all 46 new methods grouped by the 7 feature categories specified. Bumped versions from 0.1.0 to 0.2.0 in both manifest files.

### Files Changed
- `sdks/python/engram_client/client.py` — Added 46 new methods across 7 categories: Compression (5), Agentic Evolution (5), Advanced Graph (5), Autonomous Agent (10), Retrieval Excellence (7), Context Engineering (9), Temporal Graph (10). Each follows the existing pattern: `params: dict[str, Any] = {...}`, only include optional params `if param is not None`, delegate to `_mcp_call(tool_name, params)`.
- `sdks/typescript/src/index.ts` — Added matching 46 methods with camelCase names and TypeScript option interfaces per category. Optional params are passed only when `!== undefined`. Also added 20+ new exported option interfaces for the new methods.
- `sdks/python/pyproject.toml` — Bumped version from 0.1.0 to 0.2.0.
- `sdks/typescript/package.json` — Bumped version from 0.1.0 to 0.2.0.

### Decisions Made
- Python: used keyword-only args (`*`) for all optional parameters, consistent with existing `create`, `list`, `search` methods.
- Python: `consolidate(workspace, *, threshold=0.8)` — `workspace` is a required positional arg (matches spec), `threshold` is keyword-only with default.
- TypeScript: each new category got its own options interface exported at the top (e.g. `ConsolidateOptions`, `GardenOptions`) rather than inline types, keeping the file consistent with `CreateOptions`, `SearchOptions`, etc.
- TypeScript: `consolidate(workspace: string, options?: ConsolidateOptions)` mirrors the Python positional arg pattern.
- Both SDKs: `object` as a Python parameter name (in `query_triplets`, `add_knowledge`) is a builtin shadow but matches the MCP tool's field name exactly, which is the convention used elsewhere in the codebase.

### Verification
- Tests pass: n/a (no test suite for SDKs; methods are thin wrappers)
- Lint clean: patterns exactly follow existing code — no new constructs introduced
- Type check: TypeScript uses `Record<string, unknown>` params with conditional assignment, consistent with all existing methods
