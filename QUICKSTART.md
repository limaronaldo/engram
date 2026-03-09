<!-- AUTO-MAINTAINED by claude-primer. Manual edits below the marker will be preserved. -->

# Quick Start — Command Reference

Commands only. For context: [CLAUDE.md](CLAUDE.md). For rules: [STANDARDS.md](STANDARDS.md).

---

<!-- [inferred] -->
## Setup

```bash
pip install -r requirements.txt
npm install
cargo build
```

## Test

```bash
pytest
npm test
cargo test
```

## Quick Fixes

| Problem | Fix |
|---------|-----|
| Module not found | `pip install -r requirements.txt` |
| Module not found | `rm -rf node_modules && npm install` |
| Port in use | `npm run dev -- -p 3001` |

## Complete Workflow Example

A typical development cycle from start to finish:

```bash
# 1. Set up (first time only)
python -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

# 2. Create a branch for your work
git checkout -b feature/my-feature

# 3. Make changes, then verify
pytest  # run tests

# 4. Commit and push
git add -A && git commit -m 'feat: describe your change'
git push -u origin feature/my-feature
```

## References

- [CLAUDE.md](CLAUDE.md)
- [STANDARDS.md](STANDARDS.md)
- [ERRORS_AND_LESSONS.md](ERRORS_AND_LESSONS.md)

---
**Last Updated:** 2026-03-09
