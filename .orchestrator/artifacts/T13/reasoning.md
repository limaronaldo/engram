## Task: T13 — Memory Agent Loop (RML-1223)

### Approach

Created `src/intelligence/agent_loop.rs` with a pure observe→decide→act struct.
The agent is not a background thread — it exposes a `tick(conn)` method that the
caller invokes at the desired interval. One cycle: run `MemoryGardener` in dry-run
mode to surface prune/merge/archive candidates, run `GapDetector` to surface
acquisition suggestions, then cap the combined list at `max_actions_per_cycle`.
No DB writes are made by `tick()` itself; the caller (MCP handler) decides whether
to apply them.

### Files Changed

- `src/intelligence/agent_loop.rs` — new file: `AgentConfig`, `AgentState`,
  `AgentAction`, `AgentMetrics`, `CycleResult`, `MemoryAgent` with `new`, `start`,
  `stop`, `is_running`, `tick`, `should_garden`, `metrics`, `configure`, `status`.
  12 unit tests.
- `src/intelligence/mod.rs` — added `pub mod agent_loop;`

### Decisions Made

- Dry-run gardener inside tick: avoids any accidental DB mutation from the observe
  phase; the returned GardenReport provides all candidate action data without side
  effects.
- Garden action in the result signals to the caller that it should invoke
  MemoryGardener::garden() directly, keeping tick() side-effect-free.
- Separate counters for each action type needed to populate AgentMetrics without
  re-scanning the actions list.
- last_acquisition_at set on every tick that scanned for gaps (not just when
  suggestions were produced): accurately reflects "last time we checked".

### Verification

- Tests pass: yes (12/12)
- Lint clean: yes (no clippy warnings in new file)
- Type check: yes (compiles cleanly)
