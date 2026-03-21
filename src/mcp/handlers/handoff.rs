//! Session handoff handler — "land the plane" protocol.
//!
//! Generates structured session handoffs with bootstrap prompts for
//! seamless cross-session continuity. Inspired by Beads' land-the-plane pattern.

use serde_json::{json, Value};

use super::HandlerContext;

/// Land the plane: generate a structured session handoff.
///
/// Params:
/// - `session_id` (string, required) — session to hand off
/// - `workspace` (string, optional, default "default") — workspace scope
/// - `summary` (string, optional) — human-provided summary of what was accomplished
/// - `next_session_hints` (array of strings, optional) — hints for next session
pub fn session_land(ctx: &HandlerContext, params: Value) -> Value {
    // Step 1: Extract params
    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "session_id is required"}),
    };
    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .unwrap_or("default");
    let summary = params
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let next_hints: Vec<String> = params
        .get("next_session_hints")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    // Step 2-4: Query memories from the database
    let query_result = ctx.storage.with_connection(|conn| {
        use rusqlite::params;

        // Query open todos and issues
        let mut open_stmt = conn.prepare(
            "SELECT id, content, memory_type, tags, importance, created_at \
             FROM memories \
             WHERE workspace = ?1 \
               AND memory_type IN ('todo', 'issue') \
               AND (lifecycle_state IS NULL OR lifecycle_state != 'archived') \
             ORDER BY importance DESC, created_at DESC \
             LIMIT 50",
        )?;
        let open_items: Vec<Value> = open_stmt
            .query_map(params![workspace], |row| {
                let id: i64 = row.get(0)?;
                let content: String = row.get(1)?;
                let memory_type: String = row.get(2)?;
                let tags: String = row.get::<_, String>(3).unwrap_or_default();
                let importance: f64 = row.get::<_, f64>(4).unwrap_or(0.5);
                let created_at: String = row.get::<_, String>(5).unwrap_or_default();
                Ok(json!({
                    "id": id,
                    "content": content,
                    "memory_type": memory_type,
                    "tags": tags,
                    "importance": importance,
                    "created_at": created_at,
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Query recent decisions (last 24h)
        let mut dec_stmt = conn.prepare(
            "SELECT id, content, created_at \
             FROM memories \
             WHERE workspace = ?1 \
               AND memory_type = 'decision' \
               AND created_at > datetime('now', '-24 hours') \
             ORDER BY created_at DESC \
             LIMIT 20",
        )?;
        let decisions: Vec<Value> = dec_stmt
            .query_map(params![workspace], |row| {
                let id: i64 = row.get(0)?;
                let content: String = row.get(1)?;
                let created_at: String = row.get::<_, String>(2).unwrap_or_default();
                Ok(json!({
                    "id": id,
                    "content": content,
                    "created_at": created_at,
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Query recent session memories (last 24h)
        let mut recent_stmt = conn.prepare(
            "SELECT COUNT(*) \
             FROM memories \
             WHERE workspace = ?1 \
               AND created_at > datetime('now', '-24 hours')",
        )?;
        let recent_count: i64 = recent_stmt
            .query_row(params![workspace], |row| row.get(0))
            .unwrap_or(0);

        Ok((open_items, decisions, recent_count))
    });

    let (open_items, decisions, recent_count) = match query_result {
        Ok(data) => data,
        Err(e) => return json!({"error": format!("Failed to query memories: {}", e)}),
    };

    // Step 5: Build handoff structure
    let bootstrap_prompt = build_bootstrap_prompt(
        &session_id,
        workspace,
        &summary,
        &open_items,
        &decisions,
        &next_hints,
    );

    let handoff = json!({
        "session_id": session_id,
        "workspace": workspace,
        "summary": summary.clone().unwrap_or_else(|| format!("Session {} handoff", session_id)),
        "open_items": open_items,
        "recent_decisions": decisions,
        "memories_count": recent_count,
        "next_session_hints": next_hints,
        "bootstrap_prompt": bootstrap_prompt,
        "created_at": chrono::Utc::now().to_rfc3339(),
    });

    // Step 6: Create checkpoint memory
    let checkpoint_content =
        serde_json::to_string_pretty(&handoff).unwrap_or_else(|_| handoff.to_string());

    let checkpoint_input = crate::types::CreateMemoryInput {
        content: checkpoint_content,
        memory_type: crate::types::MemoryType::Checkpoint,
        tags: vec![
            "session-handoff".to_string(),
            format!("session:{}", session_id),
        ],
        workspace: Some(workspace.to_string()),
        importance: Some(0.9),
        ..Default::default()
    };

    let checkpoint_result = ctx
        .storage
        .with_transaction(|conn| crate::storage::queries::create_memory(conn, &checkpoint_input));

    match checkpoint_result {
        Ok(memory) => {
            json!({
                "handoff": handoff,
                "checkpoint_id": memory.id,
            })
        }
        Err(e) => json!({"error": format!("Failed to create checkpoint: {}", e)}),
    }
}

/// Build a markdown bootstrap prompt for the next session.
fn build_bootstrap_prompt(
    session_id: &str,
    workspace: &str,
    summary: &Option<String>,
    open_items: &[Value],
    decisions: &[Value],
    hints: &[String],
) -> String {
    let mut prompt = String::new();
    prompt.push_str(&format!("## Session Continuation — {}\n\n", session_id));

    if let Some(s) = summary {
        prompt.push_str(&format!("### Previous Session Summary\n{}\n\n", s));
    }

    if !open_items.is_empty() {
        prompt.push_str("### Open Items\n");
        for item in open_items {
            let content = item.get("content").and_then(|v| v.as_str()).unwrap_or("?");
            let mem_type = item
                .get("memory_type")
                .and_then(|v| v.as_str())
                .unwrap_or("todo");
            let truncated = if content.len() > 200 {
                &content[..200]
            } else {
                content
            };
            prompt.push_str(&format!("- [{}] {}\n", mem_type, truncated));
        }
        prompt.push('\n');
    }

    if !decisions.is_empty() {
        prompt.push_str("### Recent Decisions\n");
        for dec in decisions.iter().take(5) {
            let content = dec.get("content").and_then(|v| v.as_str()).unwrap_or("?");
            let truncated = if content.len() > 200 {
                &content[..200]
            } else {
                content
            };
            prompt.push_str(&format!("- {}\n", truncated));
        }
        prompt.push('\n');
    }

    if !hints.is_empty() {
        prompt.push_str("### Next Steps\n");
        for hint in hints {
            prompt.push_str(&format!("- {}\n", hint));
        }
        prompt.push('\n');
    }

    prompt.push_str(&format!(
        "Use `memory_search` in workspace '{}' for full context.\n",
        workspace
    ));
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_bootstrap_prompt_with_all_sections() {
        let summary = Some("Implemented session handoff feature".to_string());
        let open_items = vec![
            json!({"content": "Fix search ranking bug", "memory_type": "issue"}),
            json!({"content": "Add integration tests", "memory_type": "todo"}),
        ];
        let decisions = vec![
            json!({"content": "Use SQLite FTS5 for full-text search"}),
        ];
        let hints = vec![
            "Continue with graph traversal optimization".to_string(),
            "Review PR #42".to_string(),
        ];

        let prompt = build_bootstrap_prompt(
            "sess-001",
            "default",
            &summary,
            &open_items,
            &decisions,
            &hints,
        );

        assert!(prompt.contains("Session Continuation"));
        assert!(prompt.contains("sess-001"));
        assert!(prompt.contains("Implemented session handoff feature"));
        assert!(prompt.contains("Open Items"));
        assert!(prompt.contains("[issue] Fix search ranking bug"));
        assert!(prompt.contains("[todo] Add integration tests"));
        assert!(prompt.contains("Recent Decisions"));
        assert!(prompt.contains("Use SQLite FTS5"));
        assert!(prompt.contains("Next Steps"));
        assert!(prompt.contains("Continue with graph traversal"));
        assert!(prompt.contains("Review PR #42"));
        assert!(prompt.contains("memory_search"));
        assert!(prompt.contains("'default'"));
    }

    #[test]
    fn test_build_bootstrap_prompt_empty() {
        let prompt = build_bootstrap_prompt(
            "sess-empty",
            "work",
            &None,
            &[],
            &[],
            &[],
        );

        assert!(prompt.contains("sess-empty"));
        assert!(!prompt.contains("Previous Session Summary"));
        assert!(!prompt.contains("Open Items"));
        assert!(!prompt.contains("Recent Decisions"));
        assert!(!prompt.contains("Next Steps"));
        assert!(prompt.contains("memory_search"));
        assert!(prompt.contains("'work'"));
    }

    #[test]
    fn test_build_bootstrap_prompt_truncates_long_content() {
        let long_content = "x".repeat(300);
        let open_items = vec![
            json!({"content": long_content, "memory_type": "todo"}),
        ];

        let prompt = build_bootstrap_prompt(
            "sess-long",
            "default",
            &None,
            &open_items,
            &[],
            &[],
        );

        // Content should be truncated to 200 chars
        assert!(!prompt.contains(&"x".repeat(300)));
        assert!(prompt.contains(&"x".repeat(200)));
    }
}
