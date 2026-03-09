# Errors & Lessons — Mistake Catalog

Consult this file **before starting any task**. Organized by category, not chronologically.

## Format

```markdown
### [Category] Short description
**Context:** When/where this happens
**Wrong:** What we did that failed
**Right:** What actually works
**Date:** When discovered
```

## Categories

Use one of: Data Processing, Dependencies, API, Deploy, Logic, Config, Testing,
Tech Debt, Security, Performance, Fragile Areas

---

<!-- [placeholder] -->

### [Dependencies] Example: version mismatch after update
**Context:** After updating a dependency, imports or builds break
**Wrong:** Blindly updating all deps at once without testing
**Right:** Update one dependency at a time, run tests between each
**Date:** (template)

### [Config] Example: environment variable not loaded
**Context:** App fails on startup with missing config error
**Wrong:** Hardcoding the value as a workaround
**Right:** Check .env file exists, verify loading mechanism, add to .env.example
**Date:** (template)

### [Logic] Example: off-by-one in pagination
**Context:** API returns duplicate or missing items at page boundaries
**Wrong:** Using 1-based offset with 0-based index
**Right:** Standardize on 0-based indexing internally, convert at boundaries
**Date:** (template)

> **Note:** Replace these examples with real entries as errors are discovered.
> Delete the examples once you have real entries.

---

## Rationalization Table

Common excuses that lead to mistakes. If you catch yourself thinking these, stop.

| Excuse | Reality |
|--------|---------|
| "Too simple to test" | Simple code breaks. A test takes 30 seconds. |
| "I'll fix it later" | Later never comes. First fix sets the pattern. |
| "Should work now" | RUN the verification. Assumptions are bugs waiting to happen. |
| "Just a quick fix" | Quick fixes become permanent. Follow the full process. |
| "I'll test after I finish" | Tests written after code are weaker. Write them first. |
| "The agent said it succeeded" | Verify independently. Trust but verify. |
| "One more attempt should fix it" | 3+ failures = architectural problem. Step back. |
| "This doesn't need a plan" | Plans prevent wasted effort. 5 minutes of planning saves hours. |
| "I know this codebase" | Read the code anyway. Memory is unreliable. |

---

## Defense-in-Depth Debugging

After fixing any bug, validate at every layer the data passes through:

1. **Entry point** — is the input correct where it enters the system?
2. **Business logic** — does the transformation produce the right result?
3. **Environment guards** — are configs, permissions, and dependencies correct?
4. **Output verification** — does the final output match expectations?

Don't stop at the first layer that looks correct. Bugs hide behind other bugs.

