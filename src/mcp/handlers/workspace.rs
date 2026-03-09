//! Workspace tool handlers.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn workspace_list(ctx: &HandlerContext, _params: Value) -> Value {
    use crate::storage::queries::list_workspaces;

    ctx.storage
        .with_connection(|conn| {
            let workspaces = list_workspaces(conn)?;
            Ok(json!({"count": workspaces.len(), "workspaces": workspaces}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn workspace_stats(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::get_workspace_stats;

    let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(ws) => ws,
        None => return json!({"error": "workspace is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let stats = get_workspace_stats(conn, workspace)?;
            Ok(json!(stats))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn workspace_move(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::move_to_workspace;

    let id = match params.get("id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "id is required"}),
    };
    let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(ws) => ws,
        None => return json!({"error": "workspace is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let memory = move_to_workspace(conn, id, workspace)?;
            Ok(json!({"success": true, "memory": memory}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn workspace_delete(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::queries::delete_workspace;

    let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(ws) => ws,
        None => return json!({"error": "workspace is required"}),
    };
    let move_to_default = params
        .get("move_to_default")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    ctx.storage
        .with_connection(|conn| {
            let affected = delete_workspace(conn, workspace, move_to_default)?;
            Ok(json!({"success": true, "affected": affected, "move_to_default": move_to_default}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}
