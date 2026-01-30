# Why Engram is More Than a Vector Database

**Date:** January 29, 2026  
**Version:** 1.0

---

## Table of Contents

1. [The Problem with Pure Vector Search](#the-problem-with-pure-vector-search)
2. [The Similarity Trap](#the-similarity-trap)
3. [The Append-Only Graveyard](#the-append-only-graveyard)
4. [The Feedback Loop Gap](#the-feedback-loop-gap)
5. [Engram's Hybrid Approach](#engrams-hybrid-approach)
6. [The Utility Formula](#the-utility-formula)
7. [Comparison with Vector Databases](#comparison-with-vector-databases)

---

## The Problem with Pure Vector Search

Vector databases have become the default solution for AI memory systems. The pitch is simple: embed your content, store the vectors, and use cosine similarity to find related items. But this approach has fundamental limitations that become apparent at scale.

**The core assumption of vector search:**
> Similar embeddings = Similar meaning = Useful retrieval

This assumption breaks down in practice. Semantic similarity is necessary but not sufficient for useful memory retrieval.

---

## The Similarity Trap

### Relevance Does Not Equal Utility

**The Problem:** Vector similarity measures how semantically close two pieces of content are, not how useful the retrieved content will be for the current task.

**Example:**

Query: "How do I implement authentication in our API?"

A pure vector database might return:
1. "Authentication is the process of verifying identity" (similarity: 0.92)
2. "OAuth2 provides a standard for authentication flows" (similarity: 0.89)
3. "We decided to use JWT tokens with 15-minute expiry for our API" (similarity: 0.78)

Result #3 is the most useful - it's a specific decision about THIS project. But it ranks lowest because generic definitions are semantically closer to the query.

**The Pattern:**
- Generic content often has higher similarity scores
- Project-specific decisions get buried
- Historical context loses to current definitions

### How Engram Addresses This

Engram uses **multi-signal reranking** that considers:
- Recency (newer memories may be more relevant)
- Access frequency (frequently accessed = proven useful)
- Source trust (project decisions > general knowledge)
- Explicit feedback (user corrections)

---

## The Append-Only Graveyard

### No Natural Decay

**The Problem:** Most vector databases are append-only stores. Every piece of content added stays forever at the same importance level. Over time, old and outdated information accumulates, diluting search results.

**Example Timeline:**

```
Day 1:   "API uses REST with JSON payloads"
Day 30:  "Considering GraphQL migration"
Day 60:  "GraphQL migration approved"
Day 90:  "GraphQL migration complete, REST deprecated"
Day 120: Query "What API format do we use?"
```

A pure vector search returns all four documents with similar scores. The user must manually determine that Day 90 supersedes everything else.

**The Consequences:**
- Search results include contradictory information
- Users waste time reconciling conflicts
- No way to mark decisions as superseded
- Database grows without bounds

### How Engram Addresses This

Engram implements **memory lifecycle management**:

1. **Memory Tiering:** Daily memories auto-expire, permanent memories persist
2. **Supersedes Relationships:** Explicitly mark when one decision replaces another
3. **Quality Scoring:** Memories that prove useful get higher scores over time
4. **Soft Deletion:** Archived content stays available but ranks lower

```rust
// Mark a memory as superseding another
memory_link(
    source_id: 90,
    target_id: 1,
    edge_type: EdgeType::Supersedes
);

// Now queries automatically prefer memory #90 over #1
```

---

## The Feedback Loop Gap

### No Learning from Usage

**The Problem:** Vector databases don't learn from how memories are used. A memory that's retrieved and used successfully looks the same as one that's retrieved and ignored.

**What's Missing:**
- No tracking of which retrievals led to good outcomes
- No way to boost consistently useful memories
- No mechanism to demote false positives
- No usage analytics to improve retrieval

**The Ideal Scenario:**

```
Memory A: Retrieved 50 times, used in 45 responses
Memory B: Retrieved 50 times, used in 5 responses

Memory A should rank higher in future searches.
```

Pure vector databases can't make this distinction.

### How Engram Addresses This

Engram implements **feedback-aware retrieval**:

1. **Access Tracking:** Every retrieval is logged with context
2. **Importance Boosting:** `memory_boost` temporarily increases importance
3. **Quality Scoring:** Updated based on usage patterns
4. **Event System:** Tracks created, accessed, updated events

```rust
// Boost a memory that proved useful
memory_boost(
    id: 42,
    boost_amount: 0.3,
    duration_seconds: 3600  // 1 hour boost
);
```

---

## Engram's Hybrid Approach

### FTS5 + Vectors + RRF Fusion

Engram combines multiple search strategies and fuses their results using Reciprocal Rank Fusion (RRF).

**Three Search Channels:**

| Channel | Technology | Strength |
|---------|------------|----------|
| Keyword Search | SQLite FTS5 (BM25) | Exact matches, technical terms |
| Vector Search | sqlite-vec | Semantic similarity, concept matching |
| Fuzzy Search | Custom algorithm | Typo tolerance, variations |

**Why This Matters:**

```
Query: "asynch awiat rust"

- Keyword Search: No results (exact match fails)
- Vector Search: Finds "async/await" concepts
- Fuzzy Search: Finds "async await rust" variations

Combined: Strong results despite typos
```

### Reciprocal Rank Fusion (RRF)

Instead of just averaging scores, RRF rewards documents that appear in multiple result sets:

```
RRF_score(d) = sum(1 / (k + rank_i(d)))

Where:
- k = 60 (constant to prevent high ranks from dominating)
- rank_i(d) = rank of document d in result set i
```

**Example:**

| Document | BM25 Rank | Vector Rank | Fuzzy Rank | RRF Score |
|----------|-----------|-------------|------------|-----------|
| Doc A | 1 | 3 | 5 | 1/61 + 1/63 + 1/65 = 0.047 |
| Doc B | 10 | 1 | 2 | 1/70 + 1/61 + 1/62 = 0.047 |
| Doc C | 2 | 2 | - | 1/62 + 1/62 + 0 = 0.032 |

Documents appearing in all three channels rank highest.

---

## The Utility Formula

### Beyond Similarity

Engram's retrieval scoring considers multiple factors:

```
Utility = f(Similarity, Recency, Access Frequency, Feedback, Source Trust)
```

**Components:**

| Factor | Weight | Description |
|--------|--------|-------------|
| Similarity | 0.4 | Base semantic/keyword match score |
| Recency | 0.2 | Newer memories get boost (configurable decay) |
| Access Frequency | 0.15 | Frequently accessed = proven useful |
| Feedback | 0.15 | Explicit user corrections and boosts |
| Source Trust | 0.1 | Project decisions > general knowledge |

**Implementation:**

```rust
pub struct SearchResult {
    pub memory: Memory,
    pub score: f32,           // Combined utility score
    pub match_score: f32,     // Raw similarity
    pub recency_boost: f32,   // Time-based adjustment
    pub access_boost: f32,    // Usage-based adjustment
    pub feedback_boost: f32,  // Explicit feedback
    pub source_trust: f32,    // Origin credibility
}
```

### Configurable Weights

Different use cases need different balances:

| Use Case | Similarity | Recency | Access | Feedback | Trust |
|----------|------------|---------|--------|----------|-------|
| Code Search | High | Low | Medium | Low | High |
| Chat Context | Medium | High | Low | Medium | Low |
| Decision Lookup | Low | Medium | Medium | High | High |
| General Knowledge | High | Low | High | Medium | Medium |

---

## Comparison with Vector Databases

### Feature Matrix

| Feature | Vector DB | Engram |
|---------|-----------|--------|
| Semantic Search | Yes | Yes |
| Keyword Search | No | Yes (FTS5) |
| Fuzzy/Typo Tolerance | No | Yes |
| Multi-signal Ranking | No | Yes |
| Memory Lifecycle | No | Yes |
| Feedback Learning | No | Yes |
| Graph Relationships | Limited | Yes |
| Temporal Decay | No | Yes |
| Quality Scoring | No | Yes |

### When to Use What

**Use a Pure Vector Database when:**
- Content is homogeneous (all same type)
- No temporal component to queries
- Simple similarity is sufficient
- You're building a proof of concept

**Use Engram when:**
- Content spans decisions, notes, code, conversations
- Recency and context matter
- You need to track superseding information
- You want retrieval to improve over time
- You need exact keyword matches alongside semantic search

---

## Summary

Vector databases solve the wrong problem. They answer "What is similar?" when the real question is "What is useful?"

Engram bridges this gap by:

1. **Combining multiple search strategies** (FTS5 + Vectors + Fuzzy)
2. **Fusing results intelligently** (RRF over simple averaging)
3. **Tracking memory lifecycle** (tiering, expiration, supersedes)
4. **Learning from usage** (access tracking, feedback, quality scores)
5. **Considering multiple signals** (not just similarity)

The result: Memory retrieval that improves over time and returns what you actually need, not just what's semantically similar.

---

**See Also:**
- [CONTEXT_SEEDING.md](./CONTEXT_SEEDING.md) - Solving the cold start problem
- [SCHEMA.md](../SCHEMA.md) - Database schema documentation
- [ROADMAP.md](../ROADMAP.md) - Planned enhancements

---

**Last Updated:** January 29, 2026
