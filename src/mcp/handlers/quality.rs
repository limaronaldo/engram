//! Quality and salience tool handlers.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn quality_score(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{calculate_quality_score, ContextQualityConfig};

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let config = ContextQualityConfig::default();

    ctx.storage
        .with_transaction(|conn| {
            let score = calculate_quality_score(conn, id, &config)?;
            Ok(json!(score))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_report(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::generate_quality_report;

    let workspace = params.get("workspace").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let report = generate_quality_report(conn, workspace)?;
            Ok(json!(report))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_find_duplicates(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::find_near_duplicates;

    let threshold = params
        .get("threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.85) as f32;

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(100);

    ctx.storage
        .with_transaction(|conn| {
            let duplicates = find_near_duplicates(conn, threshold, limit)?;
            Ok(json!({"found": duplicates.len(), "duplicates": duplicates}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_get_duplicates(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::get_pending_duplicates;

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);

    ctx.storage
        .with_connection(|conn| {
            let duplicates = get_pending_duplicates(conn, limit)?;
            Ok(json!(duplicates))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_find_conflicts(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{detect_conflicts, ContextQualityConfig};

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let config = ContextQualityConfig::default();

    ctx.storage
        .with_transaction(|conn| {
            let conflicts = detect_conflicts(conn, id, &config)?;
            Ok(json!({"found": conflicts.len(), "conflicts": conflicts}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_get_conflicts(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::get_unresolved_conflicts;

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);

    ctx.storage
        .with_connection(|conn| {
            let conflicts = get_unresolved_conflicts(conn, limit)?;
            Ok(json!(conflicts))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_resolve_conflict(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{resolve_conflict, ResolutionType};

    let conflict_id = match params.get("conflict_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "conflict_id is required"}),
    };

    let resolution_str = match params.get("resolution").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => return json!({"error": "resolution is required"}),
    };

    let resolution_type = match resolution_str {
        "keep_a" => ResolutionType::KeepA,
        "keep_b" => ResolutionType::KeepB,
        "merge" => ResolutionType::Merge,
        "keep_both" => ResolutionType::KeepBoth,
        "delete_both" => ResolutionType::DeleteBoth,
        "false_positive" => ResolutionType::FalsePositive,
        _ => return json!({"error": format!("Invalid resolution type: {}", resolution_str)}),
    };

    let notes = params.get("notes").and_then(|v| v.as_str());

    ctx.storage
        .with_transaction(|conn| {
            resolve_conflict(conn, conflict_id, resolution_type, notes)?;
            Ok(json!({
                "conflict_id": conflict_id,
                "resolution": resolution_str,
                "resolved": true
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_source_trust(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{get_source_trust, update_source_trust};

    let source_type = match params.get("source_type").and_then(|v| v.as_str()) {
        Some(st) => st,
        None => return json!({"error": "source_type is required"}),
    };

    let source_identifier = params.get("source_identifier").and_then(|v| v.as_str());

    if let Some(trust_score) = params.get("trust_score").and_then(|v| v.as_f64()) {
        let notes = params.get("notes").and_then(|v| v.as_str());

        return ctx
            .storage
            .with_transaction(|conn| {
                update_source_trust(
                    conn,
                    source_type,
                    source_identifier,
                    trust_score as f32,
                    notes,
                )?;
                Ok(json!({
                    "source_type": source_type,
                    "source_identifier": source_identifier,
                    "trust_score": trust_score,
                    "updated": true
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}));
    }

    ctx.storage
        .with_connection(
            |conn| match get_source_trust(conn, source_type, source_identifier) {
                Ok(score) => Ok(json!(score)),
                Err(_) => Ok(json!({
                    "source_type": source_type,
                    "source_identifier": source_identifier,
                    "trust_score": 0.7,
                    "notes": "Default trust score"
                })),
            },
        )
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn quality_improve(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{calculate_quality_score, ContextQualityConfig};

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let config = ContextQualityConfig::default();

    ctx.storage
        .with_transaction(|conn| {
            let score = calculate_quality_score(conn, id, &config)?;
            Ok(json!({
                "memory_id": id,
                "current_quality": score.overall,
                "grade": score.grade.to_string(),
                "suggestions": score.suggestions,
                "component_scores": {
                    "clarity": score.clarity,
                    "completeness": score.completeness,
                    "freshness": score.freshness,
                    "consistency": score.consistency,
                    "source_trust": score.source_trust
                }
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

// ── Salience Tools ────────────────────────────────────────────────────────────

pub fn salience_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{get_memory_salience_with_feedback, SalienceConfig};

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let feedback_signal = params
        .get("feedback_signal")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as f32;
    let feedback_normalized = ((feedback_signal + 1.0) / 2.0).clamp(0.0, 1.0);

    ctx.storage
        .with_connection(|conn| {
            let config = SalienceConfig::default();
            let score = get_memory_salience_with_feedback(conn, id, &config, feedback_normalized)?;
            Ok(json!(score))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn salience_set_importance(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::set_memory_importance;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let importance = match params.get("importance").and_then(|v| v.as_f64()) {
        Some(imp) => imp as f32,
        None => return json!({"error": "importance is required"}),
    };

    ctx.storage
        .with_transaction(|conn| {
            set_memory_importance(conn, id, importance)?;
            Ok(json!({"id": id, "importance": importance, "updated": true}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn salience_boost(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::boost_memory_salience;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let boost_amount = params
        .get("boost_amount")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.2) as f32;

    ctx.storage
        .with_transaction(|conn| {
            let entry = boost_memory_salience(conn, id, boost_amount)?;
            Ok(json!(entry))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn salience_demote(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::demote_memory_salience;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let demote_amount = params
        .get("demote_amount")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.2) as f32;

    ctx.storage
        .with_transaction(|conn| {
            let entry = demote_memory_salience(conn, id, demote_amount)?;
            Ok(json!(entry))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn salience_decay_run(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{run_salience_decay_in_workspace, SalienceConfig};

    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let record_history = params
        .get("record_history")
        .and_then(|v| v.as_bool())
        .unwrap_or(!dry_run);

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    let mut config = SalienceConfig::default();

    if let Some(days) = params.get("stale_threshold_days").and_then(|v| v.as_f64()) {
        config.stale_threshold_days = days.max(1.0).round() as i64;
    } else if let Some(days) = params.get("stale_threshold").and_then(|v| v.as_f64()) {
        config.stale_threshold_days = days.max(1.0).round() as i64;
    }

    if let Some(days) = params
        .get("archive_threshold_days")
        .and_then(|v| v.as_f64())
    {
        config.archive_threshold_days = days.max(1.0).round() as i64;
    } else if let Some(days) = params.get("archive_threshold").and_then(|v| v.as_f64()) {
        config.archive_threshold_days = days.max(1.0).round() as i64;
    }

    if dry_run {
        return ctx
            .storage
            .with_connection(|conn| {
                conn.execute("BEGIN IMMEDIATE", [])?;
                let result =
                    run_salience_decay_in_workspace(conn, &config, false, workspace.as_deref());
                let _ = conn.execute("ROLLBACK", []);
                Ok(json!({"dry_run": true, "result": result?}))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}));
    }

    ctx.storage
        .with_transaction(|conn| {
            let result = run_salience_decay_in_workspace(
                conn,
                &config,
                record_history,
                workspace.as_deref(),
            )?;
            Ok(json!(result))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn salience_stats(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{get_salience_stats_in_workspace, SalienceConfig};

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    ctx.storage
        .with_connection(|conn| {
            let config = SalienceConfig::default();
            let stats = get_salience_stats_in_workspace(conn, &config, workspace.as_deref())?;
            Ok(json!(stats))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn salience_history(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::get_salience_history;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);

    ctx.storage
        .with_connection(|conn| {
            let history = get_salience_history(conn, id, limit)?;
            Ok(json!(history))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn salience_top(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{SalienceCalculator, SalienceConfig};
    use crate::storage::queries::list_memories;
    use crate::types::ListOptions;

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20) as usize;

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    let config = SalienceConfig::default();
    let calculator = SalienceCalculator::new(config);

    ctx.storage
        .with_connection(|conn| {
            let options = ListOptions {
                workspace: workspace.clone(),
                limit: Some(1000),
                ..Default::default()
            };
            let memories = list_memories(conn, &options)?;
            let mut scored = calculator.priority_queue(&memories);
            scored.truncate(limit);
            let items: Vec<Value> = scored
                .into_iter()
                .map(|s| {
                    json!({
                        "id": s.memory.id,
                        "content": s.memory.content,
                        "memory_type": s.memory.memory_type,
                        "workspace": s.memory.workspace,
                        "importance": s.memory.importance,
                        "salience": s.salience
                    })
                })
                .collect();
            Ok(json!({"count": items.len(), "memories": items}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
