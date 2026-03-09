//! Cloud sync, advanced sync, and multi-agent sharing tool handlers.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn sync_status(ctx: &HandlerContext, _params: Value) -> Value {
    #[cfg(feature = "cloud")]
    {
        use crate::sync::get_sync_status;
        ctx.storage
            .with_connection(|conn| {
                let status = get_sync_status(conn)?;
                Ok(json!(status))
            })
            .unwrap_or_else(|e| json!({"error": e.to_string()}))
    }
    #[cfg(not(feature = "cloud"))]
    {
        let _ = ctx;
        json!({"error": "Cloud sync requires the 'cloud' feature to be enabled"})
    }
}

pub fn sync_version(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::get_sync_version;

    ctx.storage
        .with_connection(|conn| {
            let version = get_sync_version(conn)?;
            Ok(json!(version))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn sync_delta(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::get_sync_delta;

    let since_version = match params.get("since_version").and_then(|v| v.as_i64()) {
        Some(v) => v,
        None => return json!({"error": "since_version is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let delta = get_sync_delta(conn, since_version)?;
            Ok(json!(delta))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn sync_state(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::{get_agent_sync_state, update_agent_sync_state};

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "agent_id is required"}),
    };

    // If update_version is provided, update the state first
    if let Some(version) = params.get("update_version").and_then(|v| v.as_i64()) {
        if let Err(e) = ctx
            .storage
            .with_connection(|conn| update_agent_sync_state(conn, agent_id, version))
        {
            return json!({"error": e.to_string()});
        }
    }

    ctx.storage
        .with_connection(|conn| {
            let state = get_agent_sync_state(conn, agent_id)?;
            Ok(json!(state))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn sync_cleanup(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::cleanup_sync_data;

    let older_than_days = params
        .get("older_than_days")
        .and_then(|v| v.as_i64())
        .unwrap_or(30);

    ctx.storage
        .with_connection(|conn| {
            let deleted = cleanup_sync_data(conn, older_than_days)?;
            Ok(json!({"deleted": deleted}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_share(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::share_memory;

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    let from_agent = match params.get("from_agent").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "from_agent is required"}),
    };

    let to_agent = match params.get("to_agent").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "to_agent is required"}),
    };

    let message = params.get("message").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let share_id = share_memory(conn, memory_id, from_agent, to_agent, message)?;
            Ok(json!({"share_id": share_id}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_shared_poll(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::poll_shared_memories;

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "agent_id is required"}),
    };

    let include_acknowledged = params
        .get("include_acknowledged")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    ctx.storage
        .with_connection(|conn| {
            let shares = poll_shared_memories(conn, agent_id, include_acknowledged)?;
            Ok(json!({"shares": shares}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_share_ack(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::acknowledge_share;

    let share_id = match params.get("share_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "share_id is required"}),
    };

    let agent_id = match params.get("agent_id").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "agent_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            acknowledge_share(conn, share_id, agent_id)?;
            Ok(json!({"acknowledged": true}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_events_poll(ctx: &HandlerContext, params: Value) -> Value {
    use chrono::DateTime;
    use crate::storage::poll_events;

    let since_id = params.get("since_id").and_then(|v| v.as_i64());
    let since_time = params
        .get("since_time")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));
    let agent_id = params.get("agent_id").and_then(|v| v.as_str());
    let limit = params
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    ctx.storage
        .with_connection(|conn| {
            let events = poll_events(conn, since_id, since_time, agent_id, limit)?;
            Ok(json!({"events": events}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_events_clear(ctx: &HandlerContext, params: Value) -> Value {
    use chrono::DateTime;
    use crate::storage::clear_events;

    let before_id = params.get("before_id").and_then(|v| v.as_i64());
    let before_time = params
        .get("before_time")
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc));
    let keep_recent = params
        .get("keep_recent")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);

    ctx.storage
        .with_connection(|conn| {
            let deleted = clear_events(conn, before_id, before_time, keep_recent)?;
            Ok(json!({"deleted": deleted}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
