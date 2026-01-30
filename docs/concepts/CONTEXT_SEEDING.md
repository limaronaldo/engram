# Context Seeding: Solving the Cold Start Problem

**Date:** January 29, 2026  
**Version:** 1.0

---

## Table of Contents

1. [The Cold Start Problem](#the-cold-start-problem)
2. [What is Context Seeding?](#what-is-context-seeding)
3. [The context_seed MCP Tool](#the-context_seed-mcp-tool)
4. [Dynamic TTL by Confidence](#dynamic-ttl-by-confidence)
5. [Seed Categories](#seed-categories)
6. [Seed Tagging and Metadata](#seed-tagging-and-metadata)
7. [Retrieval Priority](#retrieval-priority)
8. [Best Practices](#best-practices)
9. [Examples](#examples)

---

## The Cold Start Problem

### The Challenge

When an AI agent starts working with a new user or project, it has no context. Every interaction begins from scratch:

- "What's your preferred coding style?"
- "Which framework are you using?"
- "What's your team's naming convention?"

This creates friction and reduces efficiency. The agent must re-learn the same information repeatedly.

### Why Existing Solutions Fall Short

**Approach 1: System Prompts**
- Limited by context window size
- Static (doesn't evolve)
- One-size-fits-all

**Approach 2: Manual Memory Creation**
- Requires user effort
- Easy to forget important context
- No structure or consistency

**Approach 3: Project File Scanning**
- Captures technical context
- Misses user preferences and behaviors
- No confidence scoring

---

## What is Context Seeding?

Context seeding is a mechanism to bootstrap an AI agent's memory with initial knowledge about a user, project, or domain. Seeds are:

1. **Pre-populated memories** created from external sources
2. **Confidence-scored** based on the source's reliability
3. **Automatically managed** with TTLs based on confidence
4. **Lower priority** than organic (user-created) memories
5. **Evolvable** - can be promoted, updated, or expire

### The Seeding Workflow

```
External Source          Engram Memory System
      |                          |
      | 1. Extract facts         |
      |------------------------->|
      |                          | 2. Score confidence
      | 3. Assign TTL            |
      |<-------------------------|
      |                          | 4. Tag as seed
      | 5. Store with metadata   |
      |------------------------->|
      |                          |
      |   [Time passes...]       |
      |                          |
      | 6. Validate/Invalidate   |
      |<-------------------------|
      |                          | 7. Promote or expire
```

---

## The context_seed MCP Tool

### Tool Definition

```json
{
  "name": "context_seed",
  "description": "Seed memory with initial context from external sources",
  "parameters": {
    "content": {
      "type": "string",
      "description": "The content to seed as memory"
    },
    "category": {
      "type": "string",
      "enum": ["fact", "behavior_instruction", "interest", "persona", "preference"],
      "description": "Category of the seeded information"
    },
    "confidence": {
      "type": "number",
      "minimum": 0.0,
      "maximum": 1.0,
      "description": "Confidence score (0.0-1.0) in the accuracy of this information"
    },
    "source": {
      "type": "string",
      "description": "Where this information came from (e.g., 'github_profile', 'linkedin', 'manual')"
    },
    "ttl_strategy": {
      "type": "string",
      "enum": ["confidence_based", "fixed", "permanent"],
      "default": "confidence_based",
      "description": "How to determine TTL for this seed"
    },
    "workspace": {
      "type": "string",
      "description": "Optional workspace to scope this seed to"
    }
  }
}
```

### Response

```json
{
  "id": 42,
  "ttl_days": 90,
  "tier": "daily",
  "tags": ["origin:seed", "status:unverified", "category:preference"],
  "expires_at": "2026-04-29T00:00:00Z"
}
```

---

## Dynamic TTL by Confidence

### The Core Idea

Seeds with higher confidence scores deserve longer lifetimes. Low-confidence seeds should expire quickly unless validated by usage.

### Option C: Default Strategy

Engram uses **Option C** as the default TTL strategy:

| Confidence Range | TTL | Tier | Rationale |
|------------------|-----|------|-----------|
| >= 0.85 | Permanent | `permanent` | High confidence = likely accurate |
| 0.60 - 0.84 | 90 days | `daily` | Medium confidence = needs validation |
| < 0.60 | 30 days | `daily` | Low confidence = quick expiration |

### Implementation

```rust
fn calculate_seed_ttl(confidence: f32, strategy: TtlStrategy) -> SeedTtl {
    match strategy {
        TtlStrategy::ConfidenceBased => {
            if confidence >= 0.85 {
                SeedTtl::Permanent
            } else if confidence >= 0.60 {
                SeedTtl::Days(90)
            } else {
                SeedTtl::Days(30)
            }
        }
        TtlStrategy::Fixed(days) => SeedTtl::Days(days),
        TtlStrategy::Permanent => SeedTtl::Permanent,
    }
}
```

### Alternative Strategies

**Option A: Aggressive Expiration**
```
>= 0.90 → permanent
0.70-0.89 → 60 days
< 0.70 → 14 days
```
Use when: Sources are unreliable, rapid iteration needed

**Option B: Conservative Expiration**
```
>= 0.80 → permanent
0.50-0.79 → 180 days
< 0.50 → 60 days
```
Use when: Sources are generally reliable, slow changes expected

---

## Seed Categories

### 1. Fact

Objective information that can be verified.

**Examples:**
- "User's GitHub username is @ronaldo"
- "Project uses TypeScript 5.0"
- "Team is in Brazil timezone (UTC-3)"

**Confidence Sources:**
- API data: 0.95
- Profile page: 0.85
- Inferred: 0.60

### 2. Behavior Instruction

How the user wants the agent to behave.

**Examples:**
- "Always use TypeScript strict mode"
- "Prefer functional programming style"
- "Add JSDoc comments to public functions"

**Confidence Sources:**
- Explicit statement: 0.95
- CLAUDE.md file: 0.90
- Inferred from code: 0.70

### 3. Interest

Topics the user cares about.

**Examples:**
- "Interested in Rust systems programming"
- "Following AI/ML developments"
- "Working on real estate tech"

**Confidence Sources:**
- Starred repos: 0.75
- Blog posts: 0.80
- Explicit mention: 0.90

### 4. Persona

User's role, expertise, and communication style.

**Examples:**
- "Senior backend engineer"
- "Prefers concise explanations"
- "Experienced with distributed systems"

**Confidence Sources:**
- LinkedIn profile: 0.85
- Self-description: 0.90
- Inferred from questions: 0.60

### 5. Preference

User's choices and tastes.

**Examples:**
- "Prefers dark mode"
- "Uses VS Code as primary editor"
- "Likes detailed code comments"

**Confidence Sources:**
- Settings file: 0.90
- Explicit statement: 0.95
- Behavioral pattern: 0.65

---

## Seed Tagging and Metadata

### Automatic Tags

All seeds receive these tags automatically:

| Tag | Description |
|-----|-------------|
| `origin:seed` | Marks this as seeded (not organic) |
| `status:unverified` | Not yet validated by usage |
| `category:{category}` | The seed category |
| `source:{source}` | Where the seed came from |

### Status Transitions

```
status:unverified
       |
       | [Used in response, user didn't correct]
       v
status:validated
       |
       | [Explicitly confirmed by user]
       v
status:confirmed
```

```
status:unverified
       |
       | [User corrected or contradicted]
       v
status:invalidated
       |
       | [Auto-cleanup]
       v
[DELETED]
```

### Metadata Fields

```json
{
  "seed_source": "github_profile",
  "seed_confidence": 0.85,
  "seed_created_at": "2026-01-29T10:00:00Z",
  "seed_expires_at": "2026-04-29T10:00:00Z",
  "seed_validation_count": 0,
  "seed_invalidation_count": 0
}
```

---

## Retrieval Priority

### The Priority Hierarchy

Seeds have **lower retrieval priority** than organic memories. This ensures user-created content takes precedence.

**Priority Order (highest to lowest):**

1. **Confirmed organic memories** (user created + validated)
2. **Organic memories** (user created)
3. **Confirmed seeds** (seeded + explicitly confirmed)
4. **Validated seeds** (seeded + used without correction)
5. **Unverified seeds** (seeded + never used)

### Implementation

The search reranker applies a priority multiplier:

```rust
fn apply_priority_multiplier(memory: &Memory, base_score: f32) -> f32 {
    let multiplier = match (memory.is_seed(), memory.validation_status()) {
        (false, Status::Confirmed) => 1.0,    // Organic confirmed
        (false, _) => 0.95,                   // Organic
        (true, Status::Confirmed) => 0.90,   // Seed confirmed
        (true, Status::Validated) => 0.80,   // Seed validated
        (true, Status::Unverified) => 0.60,  // Seed unverified
        (true, Status::Invalidated) => 0.0,  // Seed invalidated (excluded)
    };
    
    base_score * multiplier
}
```

### Practical Effect

When a user asks "What's my preferred coding style?":

1. If they explicitly told Engram: that memory ranks highest
2. If Engram inferred from their code: ranks second
3. If seeded from a profile: ranks third

Seeds fill gaps but never override direct user input.

---

## Best Practices

### 1. Source Quality Mapping

Create a confidence map for your sources:

```rust
const SOURCE_CONFIDENCE: &[(&str, f32)] = &[
    ("user_explicit", 0.95),
    ("claude_md", 0.90),
    ("github_api", 0.85),
    ("linkedin_scrape", 0.80),
    ("stackoverflow_profile", 0.75),
    ("inferred_from_code", 0.70),
    ("inferred_from_behavior", 0.65),
    ("third_party_api", 0.60),
    ("web_scrape", 0.50),
];
```

### 2. Seed Incrementally

Don't dump everything at once. Seed progressively:

```
Session 1: Basic facts (name, timezone, role)
Session 2: Technical preferences (language, framework)
Session 3: Behavioral preferences (style, verbosity)
Session 4: Domain knowledge (project context)
```

### 3. Validate Early

Check seeds in the first few interactions:

```
Agent: "I see you prefer TypeScript. Should I use strict mode?"
User: "Yes, always strict mode."
[Seed validated + enhanced]
```

### 4. Handle Contradictions

When user input contradicts a seed:

1. Mark seed as `status:invalidated`
2. Create organic memory with user's correction
3. Log the contradiction for source quality feedback

### 5. Expire Gracefully

Before a seed expires, consider:

- Has it ever been used? (If not, safe to expire)
- Was it validated? (If yes, promote to permanent)
- Is there a newer memory on this topic? (If yes, let it expire)

---

## Examples

### Example 1: Seeding from GitHub Profile

```json
// Input
{
  "content": "User's primary language is Rust based on repository statistics",
  "category": "fact",
  "confidence": 0.80,
  "source": "github_api"
}

// Result
{
  "id": 101,
  "ttl_days": 90,
  "tier": "daily",
  "tags": ["origin:seed", "status:unverified", "category:fact", "source:github_api"]
}
```

### Example 2: Seeding from CLAUDE.md

```json
// Input
{
  "content": "Always use conventional commits format",
  "category": "behavior_instruction",
  "confidence": 0.90,
  "source": "claude_md"
}

// Result
{
  "id": 102,
  "ttl_days": null,  // permanent
  "tier": "permanent",
  "tags": ["origin:seed", "status:unverified", "category:behavior_instruction", "source:claude_md"]
}
```

### Example 3: Low-Confidence Inference

```json
// Input
{
  "content": "User may prefer verbose explanations based on question patterns",
  "category": "preference",
  "confidence": 0.55,
  "source": "inferred_from_behavior"
}

// Result
{
  "id": 103,
  "ttl_days": 30,
  "tier": "daily",
  "tags": ["origin:seed", "status:unverified", "category:preference", "source:inferred_from_behavior"]
}
```

### Example 4: Workspace-Scoped Seed

```json
// Input
{
  "content": "This project uses PostgreSQL with TimescaleDB extension",
  "category": "fact",
  "confidence": 0.95,
  "source": "docker_compose",
  "workspace": "ibvi-api"
}

// Result
{
  "id": 104,
  "ttl_days": null,  // permanent
  "tier": "permanent",
  "workspace": "ibvi-api",
  "tags": ["origin:seed", "status:unverified", "category:fact", "source:docker_compose"]
}
```

---

## Summary

Context seeding solves the cold start problem by:

1. **Bootstrapping memory** with external information
2. **Scoring confidence** based on source reliability
3. **Managing lifecycle** with dynamic TTLs
4. **Maintaining hierarchy** where organic memories take precedence
5. **Enabling validation** through usage and explicit confirmation

The result: AI agents that start informed but remain humble, using seeds as hints rather than ground truth.

---

**See Also:**
- [WHY_NOT_VECTOR_DB.md](./WHY_NOT_VECTOR_DB.md) - Why hybrid search matters
- [SCHEMA.md](../SCHEMA.md) - Database schema for seeds
- [ROADMAP.md](../ROADMAP.md) - Planned seeding enhancements

---

**Last Updated:** January 29, 2026
