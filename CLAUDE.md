---
project: engram
stack: python, node, rust
framework: axum
tier: T1
generated_by: claude-primer v1.8.1
last_updated: 2026-03-09
---

# CLAUDE.md

<!-- Target: keep this file under 300 lines. Split detail into STANDARDS.md or local CLAUDE.md files. -->

This file provides guidance to Claude Code when working in this repository.

**Quick reference:** [QUICKSTART.md](QUICKSTART.md)
**Standards:** [STANDARDS.md](STANDARDS.md)
**Mistakes:** [ERRORS_AND_LESSONS.md](ERRORS_AND_LESSONS.md)

---

## Routing Rules

If the task is inside a subdirectory that has its own CLAUDE.md:
1. **Read the local CLAUDE.md first** — it is the primary source for that scope.
2. Use this root file only as general context.
3. If the local file conflicts with this file, **the local file wins**.

---

## Repository Overview

Hybrid search, knowledge graphs, and optional cloud sync — shipped as a single Rust binary.

**Tech stack:** python, node, rust
**Frameworks:** axum
**Suggested tier:** T1 (medium confidence) — review required
**Tier rationale:** deploy platform detected; external-facing framework
**Deploy:** docker, github_actions

### Directory Structure

```
engram/
├── benches/
├── data/
├── docs/
├── sdks/
├── specs/
├── src/
├── tests/
├── README.md
├── CONTRIBUTING.md
├── CHANGELOG.md
```

---

## Environment

<!-- [inferred] -->
- **Python:** 3.11+ recommended
- **Node.js:** 18+ recommended
- **Rust:** stable toolchain

---

## Common Commands

<!-- [inferred] -->
```bash
pip install -r requirements.txt  # or: pip install -e .
```

```bash
npm install
```

```bash
cargo build
cargo test
cargo run
```

---

## Testing

Test directories: specs, tests

```bash
pytest
npm test
cargo test
```

---

## Code Architecture

<!-- [placeholder] -->

### Patterns
<!-- Ex: MVC, Clean Architecture, Event-driven, Layered, etc. -->

### Key Modules
<!-- List the main modules/packages and their responsibilities -->

### Data Flow
<!-- Describe the primary data flow of the application -->

---

## Invariants

> **Iron Law:** Read before writing. Understand existing code before changing it.

- Validate external input at system boundaries
- Never silently swallow errors — log or propagate with context
- Prefer dry-run for operations with external side effects
- Document decisions that affect future tasks
- Read local CLAUDE.md before modifying scoped code

---

## Decision Heuristics

When in doubt, apply these in order:

1. **Reversible over perfect** — prefer actions you can undo over waiting for certainty
2. **Smallest viable change** — solve the immediate problem, nothing more
3. **Existing patterns over new abstractions** — follow what the codebase already does
4. **Explicit failure over silent success** — if unsure something worked, make it loud
5. **Data over debate** — run the test, check the log, read the error
6. **Ask over assume** — when a decision has consequences you cannot reverse, ask the user

---

## Verification Standard

> **Iron Law:** Evidence before claims, always.

- Run the actual command — don't assume success
- Fresh verification after every change — stale results are lies
- Independent verification — don't trust agent output without checking
- Verify at every layer the data passes through (defense-in-depth)

---

## Red Flags

If you catch yourself thinking any of these, **STOP and follow the process:**

- "This is just a quick fix" → Follow the full process anyway
- "I don't need to test this" → You definitely need to test this
- "It should work now" → RUN the verification
- "One more attempt should fix it" → 3+ failures = architectural problem, step back
- "Too simple to need a plan" → Simple changes break complex systems
- "I'll clean it up later" → Later never comes. Do it right now

---

## Stuck Protocol

If you have tried **3+ approaches** to the same problem without progress:

1. **Stop** — do not attempt another fix
2. **Document** the blocker: what you tried, what failed, what you suspect
3. **List** remaining untried approaches (if any)
4. **Skip** — move to the next task or ask the user for guidance

Spinning without progress is the most expensive failure mode. Detecting it early is critical.

---

## Key Decisions

<!-- [placeholder] -->
| Decision | Rationale | Status |
|----------|-----------|--------|
| <!-- e.g. Use PostgreSQL --> | <!-- why this choice --> | <!-- Active / Revisit / Superseded --> |

<!-- Track decisions that constrain future work. Remove rows when no longer relevant. -->

---

## Active Risks

<!-- [placeholder] -->
<!-- What is currently fragile, under migration, or operationally risky -->
<!-- Remove items as they are resolved -->

---

## Formatting Standards

<!-- [placeholder] -->
- Use consistent indentation (spaces or tabs, not mixed)
- Maximum line length: 100 characters
- Files end with a single newline
- No trailing whitespace
- Use descriptive variable and function names
- Keep functions focused — one responsibility per function
- Prefer explicit over implicit

---

## Pre-Task Protocol

### Announce at Start

Before writing any code, announce:

1. **What approach** you are using (fix, feature, refactor, etc.)
2. **Which files** you expect to modify
3. **What verification** you will run when done

### Checklist

Before starting any task:

<!-- [placeholder] -->
- [ ] Read ERRORS_AND_LESSONS.md for known pitfalls
- [ ] Check if a local CLAUDE.md exists in the working directory
- [ ] Understand the existing code before making changes
- [ ] Run tests after changes to verify nothing broke
- [ ] Keep changes minimal and focused on the task
- [ ] Verify `.env` configuration is up to date

### Post-Task

Before ending a session or completing a task:

- [ ] Update ERRORS_AND_LESSONS.md if you hit a non-obvious problem
- [ ] Record any decision that constrains future work in Key Decisions
- [ ] If work is incomplete, leave a clear note about what remains
- [ ] Run final verification to confirm nothing is broken

---

## Parallel Development

Use git worktrees for parallel tasks without branch-switching conflicts:

```bash
claude --worktree feature-name    # isolated worktree + Claude session
claude -w bugfix-123 --tmux       # worktree in tmux session
git worktree list                 # see all active worktrees
```

- Each worktree gets its own branch and working directory
- Worktrees share git history — no duplicate clones
- Focus independent tasks in parallel — avoid editing same files
- Cleanup is automatic when Claude session ends without changes

---

## Provenance

Content in this file was assembled from:

- `README.md`

Sections containing `migrated` in a comment came from existing files — verify accuracy.
Sections containing `inferred` were detected from project structure — may need correction.
Sections containing `placeholder` need manual input.

---

## Document Information

**Last Updated:** 2026-03-09
**Generated by:** claude-primer v1.8.1
