//! Scope-based access grants for multi-agent memory sharing.
//!
//! Provides grant/revoke operations on the `scope_grants` table introduced in schema v31.
//!
//! An agent may be granted `read`, `write`, or `admin` access to a specific scope path
//! (or any descendant path when combined with prefix filtering in search).

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

/// A scope access grant record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeGrant {
    pub id: i64,
    pub agent_id: String,
    pub scope_path: String,
    pub permissions: String,
    pub granted_by: Option<String>,
    pub created_at: String,
}

/// Parse a `ScopeGrant` from a rusqlite `Row`.
///
/// Columns expected in order: id, agent_id, scope_path, permissions, granted_by, created_at
fn grant_from_row(row: &rusqlite::Row) -> rusqlite::Result<ScopeGrant> {
    Ok(ScopeGrant {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        scope_path: row.get(2)?,
        permissions: row.get(3)?,
        granted_by: row.get(4)?,
        created_at: row.get(5)?,
    })
}

/// Grant (or update) access for `agent_id` on `scope_path`.
///
/// On conflict (same agent + scope), updates `permissions` and `granted_by` in place.
pub fn grant_scope_access(
    conn: &Connection,
    agent_id: &str,
    scope_path: &str,
    permissions: &str,
    granted_by: Option<&str>,
) -> Result<ScopeGrant> {
    if agent_id.trim().is_empty() {
        return Err(EngramError::InvalidInput(
            "agent_id must not be empty".to_string(),
        ));
    }
    if scope_path.trim().is_empty() {
        return Err(EngramError::InvalidInput(
            "scope_path must not be empty".to_string(),
        ));
    }

    let valid_permissions = ["read", "write", "admin"];
    if !valid_permissions.contains(&permissions) {
        return Err(EngramError::InvalidInput(format!(
            "permissions must be one of: read, write, admin — got '{}'",
            permissions
        )));
    }

    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"
        INSERT INTO scope_grants (agent_id, scope_path, permissions, granted_by, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(agent_id, scope_path) DO UPDATE SET
            permissions = excluded.permissions,
            granted_by  = excluded.granted_by
        "#,
        params![agent_id, scope_path, permissions, granted_by, now],
    )?;

    get_scope_grant(conn, agent_id, scope_path)?
        .ok_or_else(|| EngramError::Storage("Grant not found after upsert".to_string()))
}

/// Revoke the grant for `agent_id` on `scope_path`.
///
/// Returns `true` if a grant was found and deleted, `false` if none existed.
pub fn revoke_scope_access(conn: &Connection, agent_id: &str, scope_path: &str) -> Result<bool> {
    let affected = conn.execute(
        "DELETE FROM scope_grants WHERE agent_id = ?1 AND scope_path = ?2",
        params![agent_id, scope_path],
    )?;
    Ok(affected > 0)
}

/// Retrieve the grant record for a specific (agent, scope) pair.
pub fn get_scope_grant(
    conn: &Connection,
    agent_id: &str,
    scope_path: &str,
) -> Result<Option<ScopeGrant>> {
    conn.query_row(
        r#"
        SELECT id, agent_id, scope_path, permissions, granted_by, created_at
        FROM scope_grants
        WHERE agent_id = ?1 AND scope_path = ?2
        "#,
        params![agent_id, scope_path],
        grant_from_row,
    )
    .optional()
    .map_err(EngramError::from)
}

/// List all grants for a given agent.
pub fn list_grants_for_agent(conn: &Connection, agent_id: &str) -> Result<Vec<ScopeGrant>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, agent_id, scope_path, permissions, granted_by, created_at
        FROM scope_grants
        WHERE agent_id = ?1
        ORDER BY created_at DESC
        "#,
    )?;
    let grants = stmt
        .query_map(params![agent_id], grant_from_row)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(grants)
}

/// Check whether `agent_id` has **at least** the specified `required_permission` on `scope_path`.
///
/// Permission hierarchy: `admin` ≥ `write` ≥ `read`.
///
/// An agent also has access to a scope if it holds a grant on any **ancestor** scope
/// (i.e., a scope_path that is a strict prefix of the requested scope_path).
///
/// Returns `true` if access is granted, `false` otherwise.
pub fn check_scope_access(
    conn: &Connection,
    agent_id: &str,
    scope_path: &str,
    required_permission: &str,
) -> Result<bool> {
    // Build list of paths to check: the scope itself plus each ancestor prefix.
    // e.g., "global/org:acme/user:alice" → ["global/org:acme/user:alice", "global/org:acme", "global"]
    let mut paths: Vec<String> = vec![scope_path.to_string()];
    let mut current = scope_path.to_string();
    while let Some(pos) = current.rfind('/') {
        current = current[..pos].to_string();
        paths.push(current.clone());
    }

    // Build placeholder list for IN clause
    let placeholders: Vec<String> = paths.iter().map(|_| "?".to_string()).collect();
    let sql = format!(
        r#"
        SELECT permissions FROM scope_grants
        WHERE agent_id = ? AND scope_path IN ({})
        "#,
        placeholders.join(", ")
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(agent_id.to_string()));
    for p in &paths {
        param_values.push(Box::new(p.clone()));
    }
    let refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let permission_rows: Vec<String> = stmt
        .query_map(refs.as_slice(), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    // Check whether any of the found grants satisfies the required level.
    for perm in &permission_rows {
        if permission_satisfies(perm, required_permission) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Returns `true` if `granted` permission level is at least as permissive as `required`.
///
/// Hierarchy: admin ≥ write ≥ read
fn permission_satisfies(granted: &str, required: &str) -> bool {
    match required {
        "read" => matches!(granted, "read" | "write" | "admin"),
        "write" => matches!(granted, "write" | "admin"),
        "admin" => granted == "admin",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations::run_migrations;

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        run_migrations(&conn).expect("run migrations");
        conn
    }

    // ── Grant / revoke ────────────────────────────────────────────────────────

    #[test]
    fn test_grant_and_get() {
        let conn = in_memory_conn();
        let grant = grant_scope_access(
            &conn,
            "agent-1",
            "global/org:acme",
            "read",
            Some("admin-agent"),
        )
        .expect("grant access");

        assert_eq!(grant.agent_id, "agent-1");
        assert_eq!(grant.scope_path, "global/org:acme");
        assert_eq!(grant.permissions, "read");
        assert_eq!(grant.granted_by.as_deref(), Some("admin-agent"));
    }

    #[test]
    fn test_grant_upsert_updates_permissions() {
        let conn = in_memory_conn();
        grant_scope_access(&conn, "agent-1", "global", "read", None).expect("grant read");
        let updated =
            grant_scope_access(&conn, "agent-1", "global", "write", None).expect("grant write");
        assert_eq!(updated.permissions, "write");
    }

    #[test]
    fn test_revoke_access() {
        let conn = in_memory_conn();
        grant_scope_access(&conn, "agent-1", "global", "read", None).expect("grant");
        let revoked = revoke_scope_access(&conn, "agent-1", "global").expect("revoke");
        assert!(revoked, "revoke should return true when grant existed");

        let again = revoke_scope_access(&conn, "agent-1", "global").expect("revoke again");
        assert!(!again, "revoke should return false when no grant exists");
    }

    #[test]
    fn test_revoke_nonexistent_returns_false() {
        let conn = in_memory_conn();
        let result = revoke_scope_access(&conn, "ghost", "global").expect("no db error");
        assert!(!result);
    }

    // ── List grants ───────────────────────────────────────────────────────────

    #[test]
    fn test_list_grants_for_agent() {
        let conn = in_memory_conn();
        grant_scope_access(&conn, "agent-1", "global", "read", None).expect("grant 1");
        grant_scope_access(&conn, "agent-1", "global/org:acme", "write", None).expect("grant 2");
        grant_scope_access(&conn, "agent-2", "global", "admin", None).expect("grant agent-2");

        let grants = list_grants_for_agent(&conn, "agent-1").expect("list");
        assert_eq!(grants.len(), 2);
        let paths: Vec<&str> = grants.iter().map(|g| g.scope_path.as_str()).collect();
        assert!(paths.contains(&"global"));
        assert!(paths.contains(&"global/org:acme"));
    }

    // ── check_scope_access ────────────────────────────────────────────────────

    #[test]
    fn test_check_access_exact_match() {
        let conn = in_memory_conn();
        grant_scope_access(&conn, "agent-1", "global/org:acme", "read", None)
            .expect("grant read");
        assert!(
            check_scope_access(&conn, "agent-1", "global/org:acme", "read")
                .expect("check read exact"),
            "read should be granted"
        );
    }

    #[test]
    fn test_check_access_ancestor_propagation() {
        let conn = in_memory_conn();
        // Grant at org level — should also satisfy access at user level
        grant_scope_access(&conn, "agent-1", "global/org:acme", "write", None).expect("grant");

        let has_access = check_scope_access(
            &conn,
            "agent-1",
            "global/org:acme/user:alice",
            "write",
        )
        .expect("check");
        assert!(has_access, "org-level write grant should satisfy user-level write check");
    }

    #[test]
    fn test_check_access_insufficient_permission() {
        let conn = in_memory_conn();
        grant_scope_access(&conn, "agent-1", "global", "read", None).expect("grant read");

        let has_write = check_scope_access(&conn, "agent-1", "global", "write").expect("check");
        assert!(!has_write, "read grant should not satisfy write requirement");
    }

    #[test]
    fn test_check_access_admin_satisfies_all() {
        let conn = in_memory_conn();
        grant_scope_access(&conn, "agent-1", "global", "admin", None).expect("grant admin");

        assert!(check_scope_access(&conn, "agent-1", "global", "read").expect("read"));
        assert!(check_scope_access(&conn, "agent-1", "global", "write").expect("write"));
        assert!(check_scope_access(&conn, "agent-1", "global", "admin").expect("admin"));
    }

    #[test]
    fn test_check_access_no_grant_returns_false() {
        let conn = in_memory_conn();
        let result = check_scope_access(&conn, "nobody", "global/org:acme", "read").expect("check");
        assert!(!result, "no grant should return false");
    }

    // ── Validation ────────────────────────────────────────────────────────────

    #[test]
    fn test_grant_empty_agent_id_fails() {
        let conn = in_memory_conn();
        let err = grant_scope_access(&conn, "   ", "global", "read", None);
        assert!(err.is_err(), "empty agent_id should fail");
    }

    #[test]
    fn test_grant_invalid_permissions_fails() {
        let conn = in_memory_conn();
        let err = grant_scope_access(&conn, "agent-1", "global", "superuser", None);
        assert!(err.is_err(), "invalid permission value should fail");
    }

    // ── permission_satisfies helper ───────────────────────────────────────────

    #[test]
    fn test_permission_hierarchy() {
        assert!(permission_satisfies("admin", "read"));
        assert!(permission_satisfies("admin", "write"));
        assert!(permission_satisfies("admin", "admin"));
        assert!(permission_satisfies("write", "read"));
        assert!(permission_satisfies("write", "write"));
        assert!(!permission_satisfies("write", "admin"));
        assert!(permission_satisfies("read", "read"));
        assert!(!permission_satisfies("read", "write"));
        assert!(!permission_satisfies("read", "admin"));
    }
}
