//! MCP Resource definitions and handlers for engram
//!
//! Implements the `resources/list` and `resources/read` MCP methods.
//! Resources expose engram data as addressable URIs that MCP clients can browse.
//!
//! Supported URI patterns:
//! - `engram://stats` — global storage statistics
//! - `engram://entities` — known entities (top 100 by mention count)
//! - `engram://memory/{id}` — a single memory by numeric ID
//! - `engram://workspace/{name}` — workspace statistics
//! - `engram://workspace/{name}/memories` — paginated memories in a workspace

use serde_json::{json, Value};

use crate::mcp::protocol::ResourceTemplate;
use crate::storage::queries::{get_memory, get_stats, get_workspace_stats, list_memories};
use crate::storage::{entity_queries::list_entities, Storage};
use crate::types::ListOptions;

/// Return all resource URI templates that engram exposes.
///
/// These are returned to MCP clients via `resources/list`.
pub fn list_resources() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate {
            uri_template: "engram://stats".to_string(),
            name: "Global Statistics".to_string(),
            description: Some("Storage statistics across all workspaces".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        ResourceTemplate {
            uri_template: "engram://entities".to_string(),
            name: "Entities".to_string(),
            description: Some(
                "Known entities extracted from memories (top 100 by mention count)".to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        },
        ResourceTemplate {
            uri_template: "engram://memory/{id}".to_string(),
            name: "Memory".to_string(),
            description: Some("A single memory by numeric ID".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        ResourceTemplate {
            uri_template: "engram://workspace/{name}".to_string(),
            name: "Workspace Statistics".to_string(),
            description: Some("Statistics for a named workspace".to_string()),
            mime_type: Some("application/json".to_string()),
        },
        ResourceTemplate {
            uri_template: "engram://workspace/{name}/memories".to_string(),
            name: "Workspace Memories".to_string(),
            description: Some(
                "Paginated memories in a workspace. Supports ?limit=N&offset=N query params."
                    .to_string(),
            ),
            mime_type: Some("application/json".to_string()),
        },
    ]
}

/// Read a resource by URI and return its JSON content.
///
/// Returns `Ok(Value)` on success, or `Err(String)` with a human-readable
/// error message that will be forwarded to the MCP client.
///
/// Supported URIs:
/// - `engram://stats`
/// - `engram://entities`
/// - `engram://memory/{id}`
/// - `engram://workspace/{name}`
/// - `engram://workspace/{name}/memories[?limit=N&offset=N]`
pub fn read_resource(storage: &Storage, uri: &str) -> Result<Value, String> {
    // Strip optional query string before routing
    let (path, query) = split_uri(uri);

    if path == "engram://stats" {
        read_stats(storage)
    } else if path == "engram://entities" {
        read_entities(storage)
    } else if let Some(rest) = path.strip_prefix("engram://memory/") {
        let id: i64 = rest
            .parse()
            .map_err(|_| format!("Invalid memory ID: {}", rest))?;
        read_memory(storage, id)
    } else if let Some(rest) = path.strip_prefix("engram://workspace/") {
        // Distinguish `workspace/{name}` from `workspace/{name}/memories`
        if let Some(name) = rest.strip_suffix("/memories") {
            read_workspace_memories(storage, name, query.as_deref())
        } else {
            read_workspace(storage, rest)
        }
    } else {
        Err(format!("Unknown resource URI: {}", uri))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Split a URI into (path, query_string).
/// `engram://workspace/foo/memories?limit=10` → `("engram://workspace/foo/memories", Some("limit=10"))`
fn split_uri(uri: &str) -> (String, Option<String>) {
    match uri.find('?') {
        Some(pos) => (uri[..pos].to_string(), Some(uri[pos + 1..].to_string())),
        None => (uri.to_string(), None),
    }
}

/// Parse `limit` and `offset` from a query string of the form `key=value&key=value`.
fn parse_pagination(query: Option<&str>) -> (Option<i64>, Option<i64>) {
    let mut limit = None;
    let mut offset = None;

    if let Some(q) = query {
        for part in q.split('&') {
            if let Some((key, val)) = part.split_once('=') {
                match key {
                    "limit" => limit = val.parse().ok(),
                    "offset" => offset = val.parse().ok(),
                    _ => {}
                }
            }
        }
    }

    (limit, offset)
}

fn read_stats(storage: &Storage) -> Result<Value, String> {
    storage
        .with_connection(|conn| {
            let stats = get_stats(conn)?;
            Ok(json!(stats))
        })
        .map_err(|e| e.to_string())
}

fn read_entities(storage: &Storage) -> Result<Value, String> {
    storage
        .with_connection(|conn| {
            let entities = list_entities(conn, None, 100, 0)?;
            Ok(json!({
                "count": entities.len(),
                "entities": entities,
            }))
        })
        .map_err(|e| e.to_string())
}

fn read_memory(storage: &Storage, id: i64) -> Result<Value, String> {
    storage
        .with_connection(|conn| {
            let memory = get_memory(conn, id)?;
            Ok(json!(memory))
        })
        .map_err(|e| e.to_string())
}

fn read_workspace(storage: &Storage, name: &str) -> Result<Value, String> {
    storage
        .with_connection(|conn| {
            let stats = get_workspace_stats(conn, name)?;
            Ok(json!(stats))
        })
        .map_err(|e| e.to_string())
}

fn read_workspace_memories(
    storage: &Storage,
    name: &str,
    query: Option<&str>,
) -> Result<Value, String> {
    let (limit, offset) = parse_pagination(query);

    storage
        .with_connection(|conn| {
            let opts = ListOptions {
                workspace: Some(name.to_string()),
                limit: Some(limit.unwrap_or(50)),
                offset,
                ..Default::default()
            };
            let memories = list_memories(conn, &opts)?;
            Ok(json!({
                "workspace": name,
                "count": memories.len(),
                "limit": limit.unwrap_or(50),
                "offset": offset.unwrap_or(0),
                "memories": memories,
            }))
        })
        .map_err(|e| e.to_string())
}
