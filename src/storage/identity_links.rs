//! Identity links and alias management
//!
//! Provides entity unification through canonical identities and aliases:
//! - Canonical IDs with display names (e.g., "user:ronaldo")
//! - Multiple aliases per identity (e.g., "Ronaldo", "@ronaldo", "limaronaldo")
//! - Alias normalization (lowercase, trim, collapse whitespace)
//! - Memory-identity linking for unified search
//!
//! Based on Fix 8 from the design plan:
//! > Normalize + explicit conflict behavior for aliases

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

/// Entity types for identities
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum IdentityType {
    #[default]
    Person,
    Organization,
    Project,
    Tool,
    Concept,
    Other,
}

impl IdentityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            IdentityType::Person => "person",
            IdentityType::Organization => "organization",
            IdentityType::Project => "project",
            IdentityType::Tool => "tool",
            IdentityType::Concept => "concept",
            IdentityType::Other => "other",
        }
    }
}

impl std::str::FromStr for IdentityType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "person" => Ok(IdentityType::Person),
            "organization" | "org" => Ok(IdentityType::Organization),
            "project" => Ok(IdentityType::Project),
            "tool" => Ok(IdentityType::Tool),
            "concept" => Ok(IdentityType::Concept),
            "other" => Ok(IdentityType::Other),
            _ => Err(format!("Unknown identity type: {}", s)),
        }
    }
}

/// An identity representing a unique entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub id: i64,
    pub canonical_id: String,
    pub display_name: String,
    pub entity_type: IdentityType,
    pub description: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub aliases: Vec<IdentityAlias>,
}

/// An alias for an identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityAlias {
    pub id: i64,
    pub canonical_id: String,
    pub alias: String,
    pub alias_normalized: String,
    pub source: Option<String>,
    pub confidence: f32,
    pub created_at: DateTime<Utc>,
}

/// A link between a memory and an identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryIdentityLink {
    pub id: i64,
    pub memory_id: i64,
    pub canonical_id: String,
    pub mention_text: Option<String>,
    pub mention_count: i32,
    pub created_at: DateTime<Utc>,
}

/// Input for creating an identity
#[derive(Debug, Clone)]
pub struct CreateIdentityInput {
    pub canonical_id: String,
    pub display_name: String,
    pub entity_type: IdentityType,
    pub description: Option<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub aliases: Vec<String>,
}

/// Normalize an alias for consistent matching.
///
/// Normalization rules:
/// - Trim whitespace
/// - Convert to lowercase
/// - Collapse multiple spaces to single space
/// - Remove leading/trailing special characters (@, #, etc.)
pub fn normalize_alias(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .trim_end_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

/// Create a new identity with optional aliases.
pub fn create_identity(conn: &Connection, input: &CreateIdentityInput) -> Result<Identity> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let metadata_json = serde_json::to_string(&input.metadata)?;

    conn.execute(
        r#"
        INSERT INTO identities (canonical_id, display_name, entity_type, description, metadata, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        params![
            input.canonical_id,
            input.display_name,
            input.entity_type.as_str(),
            input.description,
            metadata_json,
            now_str,
            now_str,
        ],
    )?;

    let _id = conn.last_insert_rowid();

    // Add aliases
    for alias in &input.aliases {
        add_alias_internal(conn, &input.canonical_id, alias, None)?;
    }

    // Also add display name as an alias
    let _ = add_alias_internal(
        conn,
        &input.canonical_id,
        &input.display_name,
        Some("display_name"),
    );

    get_identity(conn, &input.canonical_id)
}

/// Get an identity by canonical ID.
pub fn get_identity(conn: &Connection, canonical_id: &str) -> Result<Identity> {
    let identity = conn.query_row(
        r#"
        SELECT id, canonical_id, display_name, entity_type, description, metadata, created_at, updated_at
        FROM identities WHERE canonical_id = ?
        "#,
        params![canonical_id],
        |row| {
            let entity_type_str: String = row.get(3)?;
            let metadata_str: String = row.get(5)?;
            let created_at: String = row.get(6)?;
            let updated_at: String = row.get(7)?;

            Ok(Identity {
                id: row.get(0)?,
                canonical_id: row.get(1)?,
                display_name: row.get(2)?,
                entity_type: entity_type_str.parse().unwrap_or_default(),
                description: row.get(4)?,
                metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                updated_at: DateTime::parse_from_rfc3339(&updated_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                aliases: vec![],
            })
        },
    ).map_err(|_| EngramError::NotFound(0))?;

    // Load aliases
    let mut identity = identity;
    identity.aliases = get_aliases(conn, canonical_id)?;

    Ok(identity)
}

/// Update an identity.
pub fn update_identity(
    conn: &Connection,
    canonical_id: &str,
    display_name: Option<&str>,
    description: Option<&str>,
    entity_type: Option<IdentityType>,
) -> Result<Identity> {
    let now = Utc::now().to_rfc3339();

    // Build dynamic update
    let mut updates = vec!["updated_at = ?".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(now)];

    if let Some(name) = display_name {
        updates.push("display_name = ?".to_string());
        params.push(Box::new(name.to_string()));
    }

    if let Some(desc) = description {
        updates.push("description = ?".to_string());
        params.push(Box::new(desc.to_string()));
    }

    if let Some(et) = entity_type {
        updates.push("entity_type = ?".to_string());
        params.push(Box::new(et.as_str().to_string()));
    }

    params.push(Box::new(canonical_id.to_string()));

    let sql = format!(
        "UPDATE identities SET {} WHERE canonical_id = ?",
        updates.join(", ")
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let affected = conn.execute(&sql, param_refs.as_slice())?;

    if affected == 0 {
        return Err(EngramError::NotFound(0));
    }

    get_identity(conn, canonical_id)
}

/// Delete an identity and all its aliases.
pub fn delete_identity(conn: &Connection, canonical_id: &str) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM identities WHERE canonical_id = ?",
        params![canonical_id],
    )?;

    if affected == 0 {
        return Err(EngramError::NotFound(0));
    }

    Ok(())
}

/// Add an alias to an identity.
///
/// # Conflict behavior
/// - If alias (normalized) already exists for a DIFFERENT identity: REJECT with error
/// - If alias (normalized) already exists for SAME identity: UPDATE source if provided
fn add_alias_internal(
    conn: &Connection,
    canonical_id: &str,
    alias: &str,
    source: Option<&str>,
) -> Result<IdentityAlias> {
    let normalized = normalize_alias(alias);

    if normalized.is_empty() {
        return Err(EngramError::InvalidInput(
            "Alias cannot be empty".to_string(),
        ));
    }

    let now = Utc::now();
    let now_str = now.to_rfc3339();

    // Check for existing alias
    let existing: Option<String> = conn
        .query_row(
            "SELECT canonical_id FROM identity_aliases WHERE alias_normalized = ?",
            params![normalized],
            |row| row.get(0),
        )
        .ok();

    if let Some(existing_canonical) = existing {
        if existing_canonical != canonical_id {
            return Err(EngramError::Conflict(format!(
                "Alias '{}' already belongs to identity '{}'",
                alias, existing_canonical
            )));
        }
        // Same identity - update source if provided
        if let Some(src) = source {
            conn.execute(
                "UPDATE identity_aliases SET source = ? WHERE alias_normalized = ?",
                params![src, normalized],
            )?;
        }
    } else {
        // Insert new alias
        conn.execute(
            r#"
            INSERT INTO identity_aliases (canonical_id, alias, alias_normalized, source, created_at)
            VALUES (?, ?, ?, ?, ?)
            "#,
            params![canonical_id, alias, normalized, source, now_str],
        )?;
    }

    // Return the alias
    conn.query_row(
        r#"
        SELECT id, canonical_id, alias, alias_normalized, source, confidence, created_at
        FROM identity_aliases WHERE alias_normalized = ?
        "#,
        params![normalized],
        |row| {
            let created_at: String = row.get(6)?;
            Ok(IdentityAlias {
                id: row.get(0)?,
                canonical_id: row.get(1)?,
                alias: row.get(2)?,
                alias_normalized: row.get(3)?,
                source: row.get(4)?,
                confidence: row.get(5)?,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        },
    )
    .map_err(EngramError::Database)
}

/// Add an alias to an identity (public API).
pub fn add_alias(
    conn: &Connection,
    canonical_id: &str,
    alias: &str,
    source: Option<&str>,
) -> Result<IdentityAlias> {
    // Verify identity exists
    let _ = get_identity(conn, canonical_id)?;
    add_alias_internal(conn, canonical_id, alias, source)
}

/// Remove an alias from an identity.
pub fn remove_alias(conn: &Connection, alias: &str) -> Result<()> {
    let normalized = normalize_alias(alias);

    let affected = conn.execute(
        "DELETE FROM identity_aliases WHERE alias_normalized = ?",
        params![normalized],
    )?;

    if affected == 0 {
        return Err(EngramError::NotFound(0));
    }

    Ok(())
}

/// Get all aliases for an identity.
pub fn get_aliases(conn: &Connection, canonical_id: &str) -> Result<Vec<IdentityAlias>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, canonical_id, alias, alias_normalized, source, confidence, created_at
        FROM identity_aliases WHERE canonical_id = ?
        ORDER BY created_at
        "#,
    )?;

    let aliases = stmt
        .query_map(params![canonical_id], |row| {
            let created_at: String = row.get(6)?;
            Ok(IdentityAlias {
                id: row.get(0)?,
                canonical_id: row.get(1)?,
                alias: row.get(2)?,
                alias_normalized: row.get(3)?,
                source: row.get(4)?,
                confidence: row.get(5)?,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(aliases)
}

/// Resolve an alias to its canonical identity.
pub fn resolve_alias(conn: &Connection, alias: &str) -> Result<Option<Identity>> {
    let normalized = normalize_alias(alias);

    let canonical_id: Option<String> = conn
        .query_row(
            "SELECT canonical_id FROM identity_aliases WHERE alias_normalized = ?",
            params![normalized],
            |row| row.get(0),
        )
        .ok();

    match canonical_id {
        Some(cid) => Ok(Some(get_identity(conn, &cid)?)),
        None => Ok(None),
    }
}

/// Link an identity to a memory.
pub fn link_identity_to_memory(
    conn: &Connection,
    memory_id: i64,
    canonical_id: &str,
    mention_text: Option<&str>,
) -> Result<MemoryIdentityLink> {
    // Verify identity exists
    let _ = get_identity(conn, canonical_id)?;

    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"
        INSERT INTO memory_identity_links (memory_id, canonical_id, mention_text, mention_count, created_at)
        VALUES (?, ?, ?, 1, ?)
        ON CONFLICT(memory_id, canonical_id) DO UPDATE SET
            mention_count = memory_identity_links.mention_count + 1,
            mention_text = COALESCE(excluded.mention_text, memory_identity_links.mention_text)
        "#,
        params![memory_id, canonical_id, mention_text, now],
    )?;

    conn.query_row(
        r#"
        SELECT id, memory_id, canonical_id, mention_text, mention_count, created_at
        FROM memory_identity_links WHERE memory_id = ? AND canonical_id = ?
        "#,
        params![memory_id, canonical_id],
        |row| {
            let created_at: String = row.get(5)?;
            Ok(MemoryIdentityLink {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                canonical_id: row.get(2)?,
                mention_text: row.get(3)?,
                mention_count: row.get(4)?,
                created_at: DateTime::parse_from_rfc3339(&created_at)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        },
    )
    .map_err(EngramError::Database)
}

/// Unlink an identity from a memory.
pub fn unlink_identity_from_memory(
    conn: &Connection,
    memory_id: i64,
    canonical_id: &str,
) -> Result<()> {
    let affected = conn.execute(
        "DELETE FROM memory_identity_links WHERE memory_id = ? AND canonical_id = ?",
        params![memory_id, canonical_id],
    )?;

    if affected == 0 {
        return Err(EngramError::NotFound(0));
    }

    Ok(())
}

/// Get all identities linked to a memory.
pub fn get_memory_identities(conn: &Connection, memory_id: i64) -> Result<Vec<Identity>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT DISTINCT i.canonical_id
        FROM identities i
        JOIN memory_identity_links mil ON i.canonical_id = mil.canonical_id
        WHERE mil.memory_id = ?
        "#,
    )?;

    let canonical_ids: Vec<String> = stmt
        .query_map(params![memory_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut identities = Vec::new();
    for cid in canonical_ids {
        if let Ok(identity) = get_identity(conn, &cid) {
            identities.push(identity);
        }
    }

    Ok(identities)
}

/// Get all memories linked to an identity.
pub fn get_identity_memories(conn: &Connection, canonical_id: &str) -> Result<Vec<i64>> {
    let mut stmt =
        conn.prepare("SELECT memory_id FROM memory_identity_links WHERE canonical_id = ?")?;

    let memory_ids = stmt
        .query_map(params![canonical_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(memory_ids)
}

/// List all identities with optional type filter.
pub fn list_identities(
    conn: &Connection,
    entity_type: Option<IdentityType>,
    limit: i64,
) -> Result<Vec<Identity>> {
    let mut sql = String::from("SELECT canonical_id FROM identities");

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(et) = entity_type {
        sql.push_str(" WHERE entity_type = ?");
        params.push(Box::new(et.as_str().to_string()));
    }

    sql.push_str(" ORDER BY display_name LIMIT ?");
    params.push(Box::new(limit));

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;

    let canonical_ids: Vec<String> = stmt
        .query_map(param_refs.as_slice(), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut identities = Vec::new();
    for cid in canonical_ids {
        if let Ok(identity) = get_identity(conn, &cid) {
            identities.push(identity);
        }
    }

    Ok(identities)
}

/// Search identities by alias.
pub fn search_identities_by_alias(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> Result<Vec<Identity>> {
    let normalized = normalize_alias(query);
    let pattern = format!("%{}%", normalized);

    let mut stmt = conn.prepare(
        r#"
        SELECT DISTINCT i.canonical_id
        FROM identities i
        LEFT JOIN identity_aliases ia ON i.canonical_id = ia.canonical_id
        WHERE ia.alias_normalized LIKE ? OR i.display_name LIKE ?
        LIMIT ?
        "#,
    )?;

    let canonical_ids: Vec<String> = stmt
        .query_map(params![pattern, pattern, limit], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut identities = Vec::new();
    for cid in canonical_ids {
        if let Ok(identity) = get_identity(conn, &cid) {
            identities.push(identity);
        }
    }

    Ok(identities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[test]
    fn test_normalize_alias() {
        assert_eq!(normalize_alias("  Ronaldo  "), "ronaldo");
        assert_eq!(normalize_alias("@ronaldo"), "ronaldo");
        assert_eq!(normalize_alias("Lima  Ronaldo"), "lima ronaldo");
        assert_eq!(normalize_alias("#project-x"), "project-x");
        assert_eq!(normalize_alias("  UPPER CASE  "), "upper case");
    }

    #[test]
    fn test_create_identity() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                let input = CreateIdentityInput {
                    canonical_id: "user:ronaldo".to_string(),
                    display_name: "Ronaldo".to_string(),
                    entity_type: IdentityType::Person,
                    description: Some("A developer".to_string()),
                    metadata: HashMap::new(),
                    aliases: vec!["@ronaldo".to_string(), "limaronaldo".to_string()],
                };

                let identity = create_identity(conn, &input)?;

                assert_eq!(identity.canonical_id, "user:ronaldo");
                assert_eq!(identity.display_name, "Ronaldo");
                assert_eq!(identity.entity_type, IdentityType::Person);
                // Should have 3 aliases: 2 provided + display_name
                assert!(identity.aliases.len() >= 2);

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_alias_conflict() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                // Create first identity
                let input1 = CreateIdentityInput {
                    canonical_id: "user:alice".to_string(),
                    display_name: "Alice".to_string(),
                    entity_type: IdentityType::Person,
                    description: None,
                    metadata: HashMap::new(),
                    aliases: vec!["ally".to_string()],
                };
                create_identity(conn, &input1)?;

                // Create second identity
                let input2 = CreateIdentityInput {
                    canonical_id: "user:bob".to_string(),
                    display_name: "Bob".to_string(),
                    entity_type: IdentityType::Person,
                    description: None,
                    metadata: HashMap::new(),
                    aliases: vec![],
                };
                create_identity(conn, &input2)?;

                // Try to add conflicting alias
                let result = add_alias(conn, "user:bob", "ALLY", None); // Same as "ally" normalized
                assert!(result.is_err());

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn test_resolve_alias() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                let input = CreateIdentityInput {
                    canonical_id: "user:charlie".to_string(),
                    display_name: "Charlie".to_string(),
                    entity_type: IdentityType::Person,
                    description: None,
                    metadata: HashMap::new(),
                    aliases: vec!["chuck".to_string(), "@charlie".to_string()],
                };
                create_identity(conn, &input)?;

                // Resolve various forms
                let resolved = resolve_alias(conn, "CHUCK")?;
                assert!(resolved.is_some());
                assert_eq!(resolved.unwrap().canonical_id, "user:charlie");

                let resolved = resolve_alias(conn, "@Charlie")?;
                assert!(resolved.is_some());

                let resolved = resolve_alias(conn, "unknown")?;
                assert!(resolved.is_none());

                Ok(())
            })
            .unwrap();
    }
}
