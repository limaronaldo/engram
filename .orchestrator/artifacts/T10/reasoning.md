## Task: T10 — OpenAI Assistants Threads Sync Adapter for Python SDK

### Approach

Created `EngramThreadStore`, a duck-typed adapter that syncs OpenAI Assistants
API thread messages into Engram session memories.  The implementation follows
the exact pattern established by `langchain.py` and `crewai.py`: no hard
import of the third-party framework package, constructor takes an
`EngramClient` instance, and all storage goes through the existing client API.

Key design decisions:
- Dedup is implemented by searching for `message:<message_id>` tag before
  storing, then verifying the metadata's `message_id` field to avoid
  false positives from full-text search.
- `_get_attr()` helper supports both attribute access (real OpenAI SDK objects)
  and dict key access (test mocks / fixtures).
- `_extract_message_text()` handles the OpenAI content-block list format
  (`[{type: "text", text: {value: "..."}}]`) and falls back to plain strings
  for test convenience.

### Files Changed

- `sdks/python/engram_client/integrations/__init__.py` — created; exports
  `EngramThreadStore`
- `sdks/python/engram_client/integrations/openai_threads.py` — created;
  main adapter implementation with `EngramThreadStore` class and internal
  helpers
- `sdks/python/tests/__init__.py` — created; empty package marker for tests
- `sdks/python/tests/test_openai_threads.py` — created; 29 mock-based tests
  covering sync, run-scoped sync, search, dedup, edge cases, and all helpers
- `sdks/python/pyproject.toml` — added `[project.optional-dependencies]`
  section with `openai = ["openai>=1.0.0"]`

### Decisions Made

- Chose duck typing over a hard `openai` import (same as `langchain.py`
  pattern) so users without the openai package can still import the module.
- Chose to verify dedup via metadata lookup after the tag search to avoid
  false positives if free-text content happens to contain the message ID.
- Chose `"openai-threads"` as the default workspace (clearly scoped, matches
  the tag scheme `thread:<id>`).
- Added `_SENTINEL` object to distinguish "attribute not present" from `None`
  values in `_get_attr()`.

### Verification

- Tests pass: yes — 29/29 (python3 -m pytest tests/test_openai_threads.py -v)
- Lint clean: yes (no unused imports, no dead code)
- Type check: yes (all annotations use `from __future__ import annotations`)
