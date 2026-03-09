//! Identity management tool handlers.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn identity_create(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::{create_identity, CreateIdentityInput, IdentityType};

    let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return json!({"error": "canonical_id is required"}),
    };

    let display_name = match params.get("display_name").and_then(|v| v.as_str()) {
        Some(name) => name.to_string(),
        None => return json!({"error": "display_name is required"}),
    };

    let entity_type = params
        .get("entity_type")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok())
        .unwrap_or(IdentityType::Person);

    let description = params
        .get("description")
        .and_then(|v| v.as_str())
        .map(String::from);

    let aliases: Vec<String> = params
        .get("aliases")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let metadata: std::collections::HashMap<String, serde_json::Value> = params
        .get("metadata")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let input = CreateIdentityInput {
        canonical_id,
        display_name,
        entity_type,
        description,
        metadata,
        aliases,
    };

    ctx.storage
        .with_connection(|conn| {
            let identity = create_identity(conn, &input)?;
            Ok(json!(identity))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_get(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::get_identity;

    let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "canonical_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let identity = get_identity(conn, canonical_id)?;
            Ok(json!(identity))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_update(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::{update_identity, IdentityType};

    let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "canonical_id is required"}),
    };

    let display_name = params.get("display_name").and_then(|v| v.as_str());
    let description = params.get("description").and_then(|v| v.as_str());
    let entity_type: Option<IdentityType> = params
        .get("entity_type")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());

    ctx.storage
        .with_connection(|conn| {
            let identity =
                update_identity(conn, canonical_id, display_name, description, entity_type)?;
            Ok(json!(identity))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_delete(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::delete_identity;

    let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "canonical_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            delete_identity(conn, canonical_id)?;
            Ok(json!({"success": true, "canonical_id": canonical_id}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_add_alias(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::add_alias;

    let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "canonical_id is required"}),
    };

    let alias = match params.get("alias").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "alias is required"}),
    };

    let source = params.get("source").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let alias_obj = add_alias(conn, canonical_id, alias, source)?;
            Ok(json!(alias_obj))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_remove_alias(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::remove_alias;

    let alias = match params.get("alias").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "alias is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            remove_alias(conn, alias)?;
            Ok(json!({"success": true, "alias": alias}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_resolve(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::resolve_alias;

    let alias = match params.get("alias").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return json!({"error": "alias is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            let identity = resolve_alias(conn, alias)?;
            Ok(json!({"found": identity.is_some(), "identity": identity}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_list(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::{list_identities, IdentityType};

    let entity_type: Option<IdentityType> = params
        .get("entity_type")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse().ok());

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);

    ctx.storage
        .with_connection(|conn| {
            let identities = list_identities(conn, entity_type, limit)?;
            Ok(json!({"count": identities.len(), "identities": identities}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_search(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::search_identities_by_alias;

    let query = match params.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return json!({"error": "query is required"}),
    };

    let limit = params.get("limit").and_then(|v| v.as_i64()).unwrap_or(20);

    ctx.storage
        .with_connection(|conn| {
            let identities = search_identities_by_alias(conn, query, limit)?;
            Ok(json!({"count": identities.len(), "identities": identities}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_link(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::link_identity_to_memory;

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "canonical_id is required"}),
    };

    let mention_text = params.get("mention_text").and_then(|v| v.as_str());

    ctx.storage
        .with_connection(|conn| {
            let link = link_identity_to_memory(conn, memory_id, canonical_id, mention_text)?;
            Ok(json!(link))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn identity_unlink(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::unlink_identity_from_memory;

    let memory_id = match params.get("memory_id").and_then(|v| v.as_i64()) {
        Some(id) => id,
        None => return json!({"error": "memory_id is required"}),
    };

    let canonical_id = match params.get("canonical_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return json!({"error": "canonical_id is required"}),
    };

    ctx.storage
        .with_connection(|conn| {
            unlink_identity_from_memory(conn, memory_id, canonical_id)?;
            Ok(json!({"success": true, "memory_id": memory_id, "canonical_id": canonical_id}))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

pub fn memory_get_identities(ctx: &HandlerContext, params: Value) -> Value {
    use crate::storage::identity_links::get_memory_identities_with_mentions;

    let memory_id = match params
        .get("id")
        .or_else(|| params.get("memory_id"))
        .and_then(|v| v.as_i64())
    {
        Some(id) => id,
        None => return json!({"error": "id is required", "identities": []}),
    };

    ctx.storage
        .with_connection(|conn| {
            let identities = get_memory_identities_with_mentions(conn, memory_id)?;
            Ok(json!({
                "memory_id": memory_id,
                "identities_count": identities.len(),
                "identities": identities
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string(), "identities": []}))
}
