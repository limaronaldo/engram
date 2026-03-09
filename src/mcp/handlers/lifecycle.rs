//! Memory lifecycle and retention policy tool handlers.

use rusqlite::params;
use serde_json::{json, Value};

use super::HandlerContext;

pub fn lifecycle_status(ctx: &HandlerContext, params: Value) -> Value {
    let workspace = params.get("workspace").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let query = if workspace.is_some() {
                "SELECT lifecycle_state, COUNT(*) as count
                 FROM memories
                 WHERE workspace = ? AND valid_to IS NULL
                 GROUP BY lifecycle_state"
            } else {
                "SELECT lifecycle_state, COUNT(*) as count
                 FROM memories
                 WHERE valid_to IS NULL
                 GROUP BY lifecycle_state"
            };

            let mut stmt = conn.prepare(query)?;
            let rows: Vec<(String, i64)> = if let Some(ws) = workspace {
                stmt.query_map(params![ws], |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?
                            .unwrap_or_else(|| "active".to_string()),
                        row.get::<_, i64>(1)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            } else {
                stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?
                            .unwrap_or_else(|| "active".to_string()),
                        row.get::<_, i64>(1)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?
            };

            let mut active = 0i64;
            let mut stale = 0i64;
            let mut archived = 0i64;

            for (state, count) in rows {
                match state.as_str() {
                    "active" => active = count,
                    "stale" => stale = count,
                    "archived" => archived = count,
                    _ => active += count,
                }
            }

            Ok(json!({
                "active": active,
                "stale": stale,
                "archived": archived,
                "total": active + stale + archived,
                "workspace": workspace
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn lifecycle_run(ctx: &HandlerContext, params: Value) -> Value {
    use chrono::{Duration, Utc};

    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let stale_days = params
        .get("stale_days")
        .and_then(|v| v.as_i64())
        .unwrap_or(30);
    let archive_days = params
        .get("archive_days")
        .and_then(|v| v.as_i64())
        .unwrap_or(90);
    let min_importance = params
        .get("min_importance")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.5) as f32;

    let stale_cutoff = (Utc::now() - Duration::days(stale_days)).to_rfc3339();
    let archive_cutoff = (Utc::now() - Duration::days(archive_days)).to_rfc3339();

    ctx.storage
        .with_connection(|conn| {
            let stale_query = if workspace.is_some() {
                "SELECT id, content FROM memories
                 WHERE workspace = ?
                   AND (lifecycle_state IS NULL OR lifecycle_state = 'active')
                   AND created_at < ?
                   AND importance < ?
                   AND access_count < 5
                   AND valid_to IS NULL"
            } else {
                "SELECT id, content FROM memories
                 WHERE (lifecycle_state IS NULL OR lifecycle_state = 'active')
                   AND created_at < ?
                   AND importance < ?
                   AND access_count < 5
                   AND valid_to IS NULL"
            };

            let stale_candidates: Vec<(i64, String)> = {
                let mut stmt = conn.prepare(stale_query)?;
                if let Some(ws) = workspace {
                    stmt.query_map(params![ws, &stale_cutoff, min_importance], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                } else {
                    stmt.query_map(params![&stale_cutoff, min_importance], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                }
            };

            let archive_query = if workspace.is_some() {
                "SELECT id, content FROM memories
                 WHERE workspace = ?
                   AND lifecycle_state = 'stale'
                   AND created_at < ?
                   AND valid_to IS NULL"
            } else {
                "SELECT id, content FROM memories
                 WHERE lifecycle_state = 'stale'
                   AND created_at < ?
                   AND valid_to IS NULL"
            };

            let archive_candidates: Vec<(i64, String)> = {
                let mut stmt = conn.prepare(archive_query)?;
                if let Some(ws) = workspace {
                    stmt.query_map(params![ws, &archive_cutoff], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                } else {
                    stmt.query_map(params![&archive_cutoff], |row| {
                        Ok((row.get(0)?, row.get(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                }
            };

            if dry_run {
                return Ok(json!({
                    "dry_run": true,
                    "would_mark_stale": stale_candidates.len(),
                    "would_archive": archive_candidates.len(),
                    "stale_candidates": stale_candidates.iter().take(10).map(|(id, content)| {
                        json!({"id": id, "preview": content.chars().take(50).collect::<String>()})
                    }).collect::<Vec<_>>(),
                    "archive_candidates": archive_candidates.iter().take(10).map(|(id, content)| {
                        json!({"id": id, "preview": content.chars().take(50).collect::<String>()})
                    }).collect::<Vec<_>>()
                }));
            }

            let mut stale_count = 0;
            let mut archive_count = 0;

            for (id, _) in &stale_candidates {
                conn.execute(
                    "UPDATE memories SET lifecycle_state = 'stale' WHERE id = ?",
                    params![id],
                )?;
                stale_count += 1;
            }

            for (id, _) in &archive_candidates {
                conn.execute(
                    "UPDATE memories SET lifecycle_state = 'archived' WHERE id = ?",
                    params![id],
                )?;
                archive_count += 1;
            }

            Ok(json!({
                "dry_run": false,
                "marked_stale": stale_count,
                "archived": archive_count
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_set_lifecycle(ctx: &HandlerContext, params: Value) -> Value {
    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };

    let state = match params.get("state").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return json!({"error": "state is required"}),
    };

    if !["active", "stale", "archived"].contains(&state) {
        return json!({"error": "state must be one of: active, stale, archived"});
    }

    ctx.storage
        .with_connection(|conn| {
            let updated = conn.execute(
                "UPDATE memories SET lifecycle_state = ? WHERE id = ? AND valid_to IS NULL",
                params![state, id],
            )?;

            if updated == 0 {
                return Ok(json!({"error": "Memory not found"}));
            }

            Ok(json!({"id": id, "lifecycle_state": state, "updated": true}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn lifecycle_config(_ctx: &HandlerContext, params: Value) -> Value {
    let stale_days = params.get("stale_days").and_then(|v| v.as_i64());
    let archive_days = params.get("archive_days").and_then(|v| v.as_i64());
    let min_importance = params.get("min_importance").and_then(|v| v.as_f64());
    let min_access_count = params.get("min_access_count").and_then(|v| v.as_i64());

    json!({
        "stale_days": stale_days.unwrap_or(30),
        "archive_days": archive_days.unwrap_or(90),
        "min_importance": min_importance.unwrap_or(0.5),
        "min_access_count": min_access_count.unwrap_or(5),
        "lifecycle_enabled": std::env::var("ENGRAM_LIFECYCLE_ENABLED")
            .map(|v| v != "false" && v != "0")
            .unwrap_or(true),
        "note": "Pass values to update configuration"
    })
}

pub fn retention_policy_set(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::set_retention_policy;

    let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(w) => w,
        None => return json!({"error": "workspace is required"}),
    };
    let max_age_days = params.get("max_age_days").and_then(|v| v.as_i64());
    let max_memories = params.get("max_memories").and_then(|v| v.as_i64());
    let compress_after_days = params.get("compress_after_days").and_then(|v| v.as_i64());
    let compress_max_importance = params
        .get("compress_max_importance")
        .and_then(|v| v.as_f64())
        .map(|f| f as f32);
    let compress_min_access = params
        .get("compress_min_access")
        .and_then(|v| v.as_i64())
        .map(|i| i as i32);
    let auto_delete_after_days = params
        .get("auto_delete_after_days")
        .and_then(|v| v.as_i64());
    let exclude_types: Option<Vec<String>> = params
        .get("exclude_types")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });

    ctx.storage
        .with_transaction(|conn| {
            let policy = set_retention_policy(
                conn,
                workspace,
                max_age_days,
                max_memories,
                compress_after_days,
                compress_max_importance,
                compress_min_access,
                auto_delete_after_days,
                exclude_types,
            )?;
            Ok(json!(policy))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn retention_policy_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_retention_policy;

    let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(w) => w,
        None => return json!({"error": "workspace is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            match get_retention_policy(conn, workspace)? {
                Some(policy) => Ok(json!(policy)),
                None => Ok(json!({
                    "workspace": workspace,
                    "policy": null,
                    "note": "No retention policy set for this workspace"
                })),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn retention_policy_list(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::queries::list_retention_policies;

    ctx.storage
        .with_connection(|conn| {
            let policies = list_retention_policies(conn)?;
            Ok(json!({"policies": policies, "count": policies.len()}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn retention_policy_delete(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::delete_retention_policy;

    let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(w) => w,
        None => return json!({"error": "workspace is required"}),
    };

    ctx.storage
        .with_transaction(|conn| {
            let deleted = delete_retention_policy(conn, workspace)?;
            Ok(json!({"deleted": deleted, "workspace": workspace}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn retention_policy_apply(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::apply_retention_policies;

    let dry_run = params
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if dry_run {
        use crate::storage::queries::list_retention_policies;
        return ctx
            .storage
            .with_connection(|conn| {
                let policies = list_retention_policies(conn)?;
                Ok(json!({
                    "dry_run": true,
                    "policies_count": policies.len(),
                    "policies": policies,
                    "note": "Set dry_run=false to apply"
                }))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}));
    }

    ctx.storage
        .with_transaction(|conn| {
            let affected = apply_retention_policies(conn)?;
            Ok(json!({"applied": true, "memories_affected": affected}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
