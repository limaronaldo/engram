//! Autonomous agent tool handlers.
//!
//! Provides MCP tools for conflict detection, coactivation reporting,
//! triplet queries, proactive suggestions, memory gardening, and
//! the autonomous agent lifecycle (tick-based).

use serde_json::{json, Value};

use super::HandlerContext;

// ── memory_detect_conflicts ───────────────────────────────────────────────────

/// Detect conflicts in the knowledge graph.
pub fn memory_detect_conflicts(ctx: &HandlerContext, params: Value) -> Value {
    use crate::graph::conflicts::{ConflictDetector, ConflictResolver};

    let save = params
        .get("save")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    ctx.storage
        .with_connection(|conn| {
            let conflicts = ConflictDetector::detect_all(conn)?;
            let count = conflicts.len();

            if save {
                for conflict in &conflicts {
                    ConflictResolver::save_conflict(conn, conflict)?;
                }
            }

            Ok(json!({
                "conflicts": conflicts,
                "count": count,
                "saved": save,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_resolve_conflict ───────────────────────────────────────────────────

/// Resolve a saved graph conflict by ID and strategy.
pub fn memory_resolve_conflict(ctx: &HandlerContext, params: Value) -> Value {
    use crate::graph::conflicts::{ConflictResolver, ResolutionStrategy};

    let conflict_id = match params.get("conflict_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "conflict_id is required"}),
    };
    let strategy_str = params
        .get("strategy")
        .and_then(|v| v.as_str())
        .unwrap_or("keep_newer");

    let strategy = match strategy_str {
        "keep_higher_confidence" => ResolutionStrategy::KeepHigherConfidence,
        "merge" => ResolutionStrategy::Merge,
        "manual" => ResolutionStrategy::Manual,
        _ => ResolutionStrategy::KeepNewer,
    };

    ctx.storage
        .with_connection(|conn| {
            let result = ConflictResolver::resolve(conn, conflict_id, strategy)?;
            Ok(json!({
                "conflict_id": result.conflict_id,
                "strategy": strategy_str,
                "edges_removed": result.edges_removed,
                "edges_kept": result.edges_kept,
                "status": "resolved",
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_coactivation_report ────────────────────────────────────────────────

/// Get coactivation graph statistics.
pub fn memory_coactivation_report(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::graph::coactivation::CoactivationTracker;

    let tracker = CoactivationTracker::new();

    ctx.storage
        .with_connection(|conn| {
            let report = tracker.report(conn)?;
            Ok(json!({
                "total_edges": report.total_edges,
                "avg_strength": report.avg_strength,
                "strongest_pairs": report.strongest_pairs,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_query_triplets ─────────────────────────────────────────────────────

/// SPARQL-like pattern query over the facts table.
pub fn memory_query_triplets(ctx: &HandlerContext, params: Value) -> Value {
    use crate::graph::triplets::{TripletMatcher, TripletPattern};

    let subject = params
        .get("subject")
        .and_then(|v| v.as_str())
        .map(String::from);
    let predicate = params
        .get("predicate")
        .and_then(|v| v.as_str())
        .map(String::from);
    let object = params
        .get("object")
        .and_then(|v| v.as_str())
        .map(String::from);

    let pattern = TripletPattern {
        subject,
        predicate,
        object,
    };

    ctx.storage
        .with_connection(|conn| {
            let facts = TripletMatcher::match_pattern(conn, &pattern)?;
            Ok(json!({
                "facts": facts,
                "count": facts.len(),
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_knowledge_stats ────────────────────────────────────────────────────

/// Aggregate statistics about the knowledge base (facts table).
pub fn memory_knowledge_stats(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::graph::triplets::TripletMatcher;

    ctx.storage
        .with_connection(|conn| {
            let stats = TripletMatcher::knowledge_stats(conn)?;
            Ok(json!({
                "total_facts": stats.total_facts,
                "unique_subjects": stats.unique_subjects,
                "unique_predicates": stats.unique_predicates,
                "unique_objects": stats.unique_objects,
                "top_predicates": stats.top_predicates,
                "top_subjects": stats.top_subjects,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_suggest_acquisitions ───────────────────────────────────────────────

/// Suggest new memories to create based on knowledge gap analysis.
pub fn memory_suggest_acquisitions(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::proactive::GapDetector;

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let limit: usize = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10);

    let detector = GapDetector::new();

    ctx.storage
        .with_connection(|conn| {
            let suggestions = detector.suggest_acquisitions(conn, workspace, limit)?;
            Ok(json!({
                "workspace": workspace,
                "suggestions": suggestions,
                "count": suggestions.len(),
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_garden ─────────────────────────────────────────────────────────────

/// Run full garden maintenance on a workspace.
pub fn memory_garden(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::gardening::{GardenConfig, MemoryGardener};

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let config = GardenConfig {
        dry_run: false,
        ..GardenConfig::default()
    };

    ctx.storage
        .with_connection(|conn| {
            let gardener = MemoryGardener::new(config);
            let report = gardener.garden(conn, workspace)?;
            Ok(json!({
                "workspace": workspace,
                "dry_run": false,
                "memories_pruned": report.memories_pruned,
                "memories_merged": report.memories_merged,
                "memories_archived": report.memories_archived,
                "memories_compressed": report.memories_compressed,
                "tokens_freed": report.tokens_freed,
                "actions": report.actions,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_garden_preview ─────────────────────────────────────────────────────

/// Dry-run garden maintenance — no changes made.
pub fn memory_garden_preview(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::gardening::{GardenConfig, MemoryGardener};

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let config = GardenConfig {
        dry_run: true,
        ..GardenConfig::default()
    };

    ctx.storage
        .with_connection(|conn| {
            let gardener = MemoryGardener::new(config);
            let report = gardener.garden(conn, workspace)?;
            Ok(json!({
                "workspace": workspace,
                "dry_run": true,
                "memories_would_prune": report.memories_pruned,
                "memories_would_merge": report.memories_merged,
                "memories_would_archive": report.memories_archived,
                "memories_would_compress": report.memories_compressed,
                "tokens_would_free": report.tokens_freed,
                "actions": report.actions,
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_agent_start ────────────────────────────────────────────────────────

/// Create and start a memory agent configuration (returns config for use with tick).
pub fn memory_agent_start(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::agent_loop::AgentConfig;

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let interval_secs: u64 = params
        .get("interval_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(300);

    let config = AgentConfig {
        workspace: workspace.to_string(),
        check_interval_secs: interval_secs,
        ..AgentConfig::default()
    };

    json!({
        "status": "configured",
        "workspace": workspace,
        "check_interval_secs": config.check_interval_secs,
        "garden_interval_secs": config.garden_interval_secs,
        "max_actions_per_cycle": config.max_actions_per_cycle,
        "note": "Use memory_agent_tick to run a cycle",
    })
}

// ── memory_agent_stop ─────────────────────────────────────────────────────────

/// Stop a memory agent (no-op for stateless tick-based agents).
pub fn memory_agent_stop(_ctx: &HandlerContext, params: Value) -> Value {
    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    json!({
        "status": "stopped",
        "workspace": workspace,
        "note": "Tick-based agent — no background thread to stop",
    })
}

// ── memory_agent_status ───────────────────────────────────────────────────────

/// Return agent status information.
pub fn memory_agent_status(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_stats;

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    ctx.storage
        .with_connection(|conn| {
            let stats = get_stats(conn)?;
            Ok(json!({
                "workspace": workspace,
                "total_memories": stats.total_memories,
                "agent_model": "tick-based",
                "status": "idle",
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── memory_agent_metrics ──────────────────────────────────────────────────────

/// Run one agent cycle and return the decided actions.
pub fn memory_agent_metrics(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::agent_loop::{AgentConfig, MemoryAgent};

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let max_actions: usize = params
        .get("max_actions")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(10);

    let config = AgentConfig {
        workspace: workspace.to_string(),
        max_actions_per_cycle: max_actions,
        ..AgentConfig::default()
    };

    let mut agent = MemoryAgent::new(config);
    agent.start();

    ctx.storage
        .with_connection(|conn| {
            let cycle = agent.tick(conn)?;
            let metrics = agent.metrics();
            Ok(json!({
                "workspace": workspace,
                "cycle_number": cycle.cycle_number,
                "duration_ms": cycle.duration_ms,
                "actions": cycle.actions,
                "metrics": {
                    "cycles": metrics.cycles,
                    "total_actions": metrics.total_actions,
                    "memories_pruned": metrics.memories_pruned,
                    "memories_merged": metrics.memories_merged,
                    "memories_archived": metrics.memories_archived,
                    "suggestions_made": metrics.suggestions_made,
                    "uptime_secs": metrics.uptime_secs,
                },
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
