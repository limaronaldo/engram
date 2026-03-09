//! Session tool handlers.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn session_index(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::session_indexing::{index_conversation, ChunkingConfig, Message};

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "session_id is required"}),
    };

    let messages: Vec<Message> = match params.get("messages").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|m| {
                let role = m.get("role")?.as_str()?.to_string();
                let content = m.get("content")?.as_str()?.to_string();
                let timestamp = m
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now);
                let id = m.get("id").and_then(|i| i.as_str()).map(String::from);
                Some(Message {
                    role,
                    content,
                    timestamp,
                    id,
                })
            })
            .collect(),
        None => return json!({"error": "messages array is required"}),
    };

    if messages.is_empty() {
        return json!({"error": "messages array cannot be empty"});
    }

    let title = params.get("title").and_then(|v| v.as_str());
    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let agent_id = params.get("agent_id").and_then(|v| v.as_str());

    let config = ChunkingConfig {
        max_messages: params
            .get("max_messages")
            .and_then(|v| v.as_i64())
            .unwrap_or(10) as usize,
        overlap_messages: params.get("overlap").and_then(|v| v.as_i64()).unwrap_or(2) as usize,
        max_chars: params
            .get("max_chars")
            .and_then(|v| v.as_i64())
            .unwrap_or(8000) as usize,
        default_ttl_seconds: params.get("ttl_days").and_then(|v| v.as_i64()).unwrap_or(7)
            * 24
            * 60
            * 60,
    };

    ctx.storage
        .with_connection(|conn| {
            let session = index_conversation(
                conn, session_id, &messages, &config, workspace, title, agent_id,
            )?;
            Ok(json!({"success": true, "session": session}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_index_delta(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::session_indexing::{
        index_conversation_delta, ChunkingConfig, Message,
    };

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "session_id is required"}),
    };

    let messages: Vec<Message> = match params.get("messages").and_then(|v| v.as_array()) {
        Some(arr) => arr
            .iter()
            .filter_map(|m| {
                let role = m.get("role")?.as_str()?.to_string();
                let content = m.get("content")?.as_str()?.to_string();
                let timestamp = m
                    .get("timestamp")
                    .and_then(|t| t.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now);
                let id = m.get("id").and_then(|i| i.as_str()).map(String::from);
                Some(Message {
                    role,
                    content,
                    timestamp,
                    id,
                })
            })
            .collect(),
        None => return json!({"error": "messages array is required"}),
    };

    let config = ChunkingConfig::default();

    ctx.storage
        .with_connection(|conn| {
            let session = index_conversation_delta(conn, session_id, &messages, &config)?;
            Ok(json!({"success": true, "session": session}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::session_indexing::get_session;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "session_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let session = get_session(conn, session_id)?;
            Ok(json!(session))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_list(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::session_indexing::list_sessions;

    let workspace = params.get("workspace").and_then(|v| v.as_str());
    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

    ctx.storage
        .with_connection(|conn| {
            let sessions = list_sessions(conn, workspace, limit)?;
            Ok(json!({"count": sessions.len(), "sessions": sessions}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_delete(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::session_indexing::delete_session;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "session_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            delete_session(conn, session_id)?;
            Ok(json!({"success": true, "session_id": session_id}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_create(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{create_session, CreateSessionInput};

    let title = params
        .get("name")
        .and_then(|v| v.as_str())
        .map(String::from);
    if title.is_none() {
        return json!({"error": "name is required"});
    }

    let initial_context = params
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);

    let metadata = params
        .get("metadata")
        .and_then(|v| v.as_object())
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<std::collections::HashMap<_, _>>()
        })
        .unwrap_or_default();

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    let input = CreateSessionInput {
        session_id: None,
        title,
        initial_context,
        workspace,
        metadata,
    };

    ctx.storage
        .with_transaction(|conn| {
            let session = create_session(conn, input)?;
            Ok(json!(session))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_add_memory(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{add_memory_to_session, ContextRole};

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    let relevance_score = params
        .get("relevance_score")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as f32;

    let context_role = params
        .get("context_role")
        .and_then(|v| v.as_str())
        .map(|s| match s.to_lowercase().as_str() {
            "created" => ContextRole::Created,
            "updated" => ContextRole::Updated,
            "pinned" => ContextRole::Pinned,
            _ => ContextRole::Referenced,
        })
        .unwrap_or(ContextRole::Referenced);

    ctx.storage
        .with_transaction(|conn| {
            let link =
                add_memory_to_session(conn, &session_id, memory_id, relevance_score, context_role)?;
            Ok(json!(link))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_remove_memory(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::remove_memory_from_session;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    ctx.storage
        .with_transaction(|conn| {
            remove_memory_from_session(conn, &session_id, memory_id)?;
            Ok(json!({"session_id": session_id, "memory_id": memory_id, "removed": true}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::get_session_context;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let context = get_session_context(conn, &session_id)?;
            Ok(json!(context))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_list(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::list_sessions_extended;

    let active_only = params
        .get("active_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let workspace = params
        .get("workspace")
        .and_then(|v| v.as_str())
        .map(String::from);

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);
    let offset = params.get("offset").and_then(|v| v.as_i64()).unwrap_or(0);

    ctx.storage
        .with_connection(|conn| {
            let sessions =
                list_sessions_extended(conn, workspace.as_deref(), active_only, limit, offset)?;
            Ok(json!(sessions))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_search(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::search_session_memories;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q.to_string(),
        None => return json!({"error": "query is required"}),
    };

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

    ctx.storage
        .with_connection(|conn| {
            let results = search_session_memories(conn, &session_id, &query, limit)?;
            Ok(json!(results))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_update_summary(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::update_session_summary;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    let summary = match params.get("summary").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => return json!({"error": "summary is required"}),
    };

    ctx.storage
        .with_transaction(|conn| {
            update_session_summary(conn, &session_id, &summary)?;
            Ok(json!({"session_id": session_id, "summary": summary, "updated": true}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_end(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{end_session, update_session_summary};

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    let summary = params.get("summary").and_then(|v| v.as_str());

    ctx.storage
        .with_transaction(|conn| {
            if let Some(summary) = summary {
                update_session_summary(conn, &session_id, summary)?;
                Ok(json!({"session_id": session_id, "summary": summary, "ended": true}))
            } else {
                end_session(conn, &session_id)?;
                Ok(json!({"session_id": session_id, "ended": true}))
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn session_context_export(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::export_session;

    let session_id = match params.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "session_id is required"}),
    };

    let format = params
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("json");
    let include_content = params
        .get("include_content")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    ctx.storage
        .with_connection(|conn| {
            let export = export_session(conn, &session_id, include_content)?;

            match format {
                "markdown" => {
                    let mut md = String::new();
                    let title = export
                        .session
                        .title
                        .as_deref()
                        .unwrap_or("Untitled Session");
                    md.push_str(&format!("# Session: {}\n\n", title));
                    if let Some(summary) = &export.session.summary {
                        md.push_str(&format!("## Summary\n\n{}\n\n", summary));
                    }
                    md.push_str(&format!(
                        "**Created:** {}\n",
                        export.session.created_at.format("%Y-%m-%d %H:%M:%S")
                    ));
                    if let Some(ended) = export.session.ended_at {
                        md.push_str(&format!(
                            "**Ended:** {}\n",
                            ended.format("%Y-%m-%d %H:%M:%S")
                        ));
                    }
                    md.push_str(&format!("\n## Memories ({})\n\n", export.memories.len()));

                    for mem in &export.memories {
                        md.push_str(&format!(
                            "### Memory #{} ({})\n\n",
                            mem.id,
                            mem.memory_type.as_str()
                        ));
                        md.push_str(&format!("{}\n\n", mem.content));
                        if !mem.tags.is_empty() {
                            md.push_str(&format!("Tags: {}\n\n", mem.tags.join(", ")));
                        }
                    }

                    Ok(json!({"format": "markdown", "content": md}))
                }
                _ => Ok(json!(export)),
            }
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
