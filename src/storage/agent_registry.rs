//! Agent registry storage queries
//!
//! Provides CRUD operations for the `agents` table introduced in schema v17.
//!
//! Agents represent registered AI agents with capabilities, namespaces,
//! heartbeat tracking, and lifecycle status.

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

/// A registered AI agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub agent_id: String,
    pub display_name: String,
    pub capabilities: Vec<String>,
    pub namespaces: Vec<String>,
    pub last_heartbeat: Option<String>,
    pub status: String,
    pub metadata: serde_json::Value,
    pub registered_at: String,
    pub updated_at: String,
}

/// Input for registering a new agent or updating an existing one (upsert)
#[derive(Debug, Clone)]
pub struct RegisterAgentInput {
    pub agent_id: String,
    pub display_name: String,
    pub capabilities: Vec<String>,
    pub namespaces: Vec<String>,
    pub metadata: serde_json::Value,
}

impl Default for RegisterAgentInput {
    fn default() -> Self {
        Self {
            agent_id: String::new(),
            display_name: String::new(),
            capabilities: vec![],
            namespaces: vec!["default".to_string()],
            metadata: serde_json::Value::Object(serde_json::Map::new()),
        }
    }
}

/// Parse an Agent from a rusqlite row.
///
/// Columns expected in order: agent_id, display_name, capabilities, namespaces,
/// last_heartbeat, status, metadata, registered_at, updated_at
fn agent_from_row(row: &rusqlite::Row) -> rusqlite::Result<Agent> {
    let capabilities_str: String = row.get(2)?;
    let namespaces_str: String = row.get(3)?;
    let metadata_str: String = row.get(6)?;

    let capabilities: Vec<String> =
        serde_json::from_str(&capabilities_str).unwrap_or_default();
    let namespaces: Vec<String> =
        serde_json::from_str(&namespaces_str).unwrap_or_else(|_| vec!["default".to_string()]);
    let metadata: serde_json::Value =
        serde_json::from_str(&metadata_str).unwrap_or(serde_json::Value::Object(Default::default()));

    Ok(Agent {
        agent_id: row.get(0)?,
        display_name: row.get(1)?,
        capabilities,
        namespaces,
        last_heartbeat: row.get(4)?,
        status: row.get(5)?,
        metadata,
        registered_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

/// Register a new agent, or update an existing one if the `agent_id` already exists.
///
/// On conflict, updates: display_name, capabilities, namespaces, metadata, updated_at.
/// The `registered_at` timestamp is preserved from the original registration.
pub fn register_agent(conn: &Connection, input: &RegisterAgentInput) -> Result<Agent> {
    if input.agent_id.trim().is_empty() {
        return Err(EngramError::InvalidInput(
            "agent_id must not be empty".to_string(),
        ));
    }
    if input.display_name.trim().is_empty() {
        return Err(EngramError::InvalidInput(
            "display_name must not be empty".to_string(),
        ));
    }

    let now = Utc::now().to_rfc3339();
    let capabilities_json = serde_json::to_string(&input.capabilities)?;
    let namespaces_json = serde_json::to_string(&input.namespaces)?;
    let metadata_json = serde_json::to_string(&input.metadata)?;

    conn.execute(
        r#"
        INSERT INTO agents
            (agent_id, display_name, capabilities, namespaces, status, metadata, registered_at, updated_at)
        VALUES (?, ?, ?, ?, 'active', ?, ?, ?)
        ON CONFLICT(agent_id) DO UPDATE SET
            display_name = excluded.display_name,
            capabilities = excluded.capabilities,
            namespaces   = excluded.namespaces,
            metadata     = excluded.metadata,
            status       = 'active',
            updated_at   = excluded.updated_at
        "#,
        params![
            input.agent_id,
            input.display_name,
            capabilities_json,
            namespaces_json,
            metadata_json,
            now,
            now,
        ],
    )?;

    get_agent(conn, &input.agent_id)?
        .ok_or_else(|| EngramError::Storage("Agent not found after insert".to_string()))
}

/// Deregister an agent by setting its status to 'inactive'.
///
/// Returns `true` if the agent was found and deregistered, `false` if not found.
pub fn deregister_agent(conn: &Connection, agent_id: &str) -> Result<bool> {
    let now = Utc::now().to_rfc3339();

    let affected = conn.execute(
        "UPDATE agents SET status = 'inactive', updated_at = ? WHERE agent_id = ?",
        params![now, agent_id],
    )?;

    Ok(affected > 0)
}

/// Update the heartbeat timestamp for an agent.
///
/// Returns the updated `Agent` if found, or `None` if the agent does not exist.
pub fn heartbeat_agent(conn: &Connection, agent_id: &str) -> Result<Option<Agent>> {
    let now = Utc::now().to_rfc3339();

    let affected = conn.execute(
        "UPDATE agents SET last_heartbeat = ?, updated_at = ? WHERE agent_id = ?",
        params![now, now, agent_id],
    )?;

    if affected == 0 {
        return Ok(None);
    }

    get_agent(conn, agent_id)
}

/// Retrieve a single agent by its ID.
pub fn get_agent(conn: &Connection, agent_id: &str) -> Result<Option<Agent>> {
    conn.query_row(
        r#"
        SELECT agent_id, display_name, capabilities, namespaces,
               last_heartbeat, status, metadata, registered_at, updated_at
        FROM agents WHERE agent_id = ?
        "#,
        params![agent_id],
        agent_from_row,
    )
    .optional()
    .map_err(EngramError::from)
}

/// List all agents, optionally filtered by status.
///
/// `status_filter` accepts values like `"active"` or `"inactive"`.
/// Pass `None` to return all agents regardless of status.
pub fn list_agents(conn: &Connection, status_filter: Option<&str>) -> Result<Vec<Agent>> {
    let (sql, param_str): (&str, Option<String>) = match status_filter {
        Some(s) => (
            r#"
            SELECT agent_id, display_name, capabilities, namespaces,
                   last_heartbeat, status, metadata, registered_at, updated_at
            FROM agents WHERE status = ?
            ORDER BY registered_at DESC
            "#,
            Some(s.to_string()),
        ),
        None => (
            r#"
            SELECT agent_id, display_name, capabilities, namespaces,
                   last_heartbeat, status, metadata, registered_at, updated_at
            FROM agents
            ORDER BY registered_at DESC
            "#,
            None,
        ),
    };

    let mut stmt = conn.prepare(sql)?;

    let agents = if let Some(ref status) = param_str {
        stmt.query_map(params![status], agent_from_row)?
            .filter_map(|r| r.ok())
            .collect()
    } else {
        stmt.query_map([], agent_from_row)?
            .filter_map(|r| r.ok())
            .collect()
    };

    Ok(agents)
}

/// Update the capabilities list for an agent.
///
/// Returns the updated `Agent` if found, or `None` if the agent does not exist.
pub fn update_agent_capabilities(
    conn: &Connection,
    agent_id: &str,
    capabilities: &[String],
) -> Result<Option<Agent>> {
    let now = Utc::now().to_rfc3339();
    let capabilities_json = serde_json::to_string(capabilities)?;

    let affected = conn.execute(
        "UPDATE agents SET capabilities = ?, updated_at = ? WHERE agent_id = ?",
        params![capabilities_json, now, agent_id],
    )?;

    if affected == 0 {
        return Ok(None);
    }

    get_agent(conn, agent_id)
}

/// List all active agents that belong to the given namespace.
pub fn get_agents_in_namespace(conn: &Connection, namespace: &str) -> Result<Vec<Agent>> {
    // SQLite JSON array membership: json_each returns rows for each element.
    let mut stmt = conn.prepare(
        r#"
        SELECT a.agent_id, a.display_name, a.capabilities, a.namespaces,
               a.last_heartbeat, a.status, a.metadata, a.registered_at, a.updated_at
        FROM agents a
        WHERE a.status = 'active'
          AND EXISTS (
              SELECT 1 FROM json_each(a.namespaces)
              WHERE value = ?
          )
        ORDER BY a.registered_at DESC
        "#,
    )?;

    let agents = stmt
        .query_map(params![namespace], agent_from_row)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(agents)
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

    fn basic_input(agent_id: &str) -> RegisterAgentInput {
        RegisterAgentInput {
            agent_id: agent_id.to_string(),
            display_name: "Test Agent".to_string(),
            capabilities: vec!["read".to_string(), "write".to_string()],
            namespaces: vec!["default".to_string()],
            metadata: serde_json::json!({"version": "1.0"}),
        }
    }

    #[test]
    fn test_register_and_get_agent() {
        let conn = in_memory_conn();
        let input = basic_input("agent-001");

        let agent = register_agent(&conn, &input).expect("register agent");
        assert_eq!(agent.agent_id, "agent-001");
        assert_eq!(agent.display_name, "Test Agent");
        assert_eq!(agent.capabilities, vec!["read", "write"]);
        assert_eq!(agent.namespaces, vec!["default"]);
        assert_eq!(agent.status, "active");
        assert!(agent.last_heartbeat.is_none());

        let fetched = get_agent(&conn, "agent-001")
            .expect("get agent")
            .expect("agent exists");
        assert_eq!(fetched.agent_id, agent.agent_id);
        assert_eq!(fetched.display_name, agent.display_name);
    }

    #[test]
    fn test_deregister_agent() {
        let conn = in_memory_conn();
        register_agent(&conn, &basic_input("agent-deregister")).expect("register");

        let found = deregister_agent(&conn, "agent-deregister").expect("deregister");
        assert!(found, "should return true for existing agent");

        let agent = get_agent(&conn, "agent-deregister")
            .expect("get")
            .expect("exists");
        assert_eq!(agent.status, "inactive");
    }

    #[test]
    fn test_heartbeat_updates_timestamp() {
        let conn = in_memory_conn();
        register_agent(&conn, &basic_input("agent-hb")).expect("register");

        let before = get_agent(&conn, "agent-hb").expect("get").expect("exists");
        assert!(before.last_heartbeat.is_none());

        let updated = heartbeat_agent(&conn, "agent-hb")
            .expect("heartbeat")
            .expect("agent found");
        assert!(
            updated.last_heartbeat.is_some(),
            "last_heartbeat should be set after heartbeat"
        );
    }

    #[test]
    fn test_list_agents_with_filter() {
        let conn = in_memory_conn();
        register_agent(&conn, &basic_input("agent-a1")).expect("register");
        register_agent(&conn, &basic_input("agent-a2")).expect("register");
        deregister_agent(&conn, "agent-a2").expect("deregister");

        let active = list_agents(&conn, Some("active")).expect("list active");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].agent_id, "agent-a1");

        let inactive = list_agents(&conn, Some("inactive")).expect("list inactive");
        assert_eq!(inactive.len(), 1);
        assert_eq!(inactive[0].agent_id, "agent-a2");

        let all = list_agents(&conn, None).expect("list all");
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_update_capabilities() {
        let conn = in_memory_conn();
        register_agent(&conn, &basic_input("agent-caps")).expect("register");

        let updated = update_agent_capabilities(
            &conn,
            "agent-caps",
            &["search".to_string(), "create".to_string(), "delete".to_string()],
        )
        .expect("update")
        .expect("found");

        assert_eq!(updated.capabilities, vec!["search", "create", "delete"]);
    }

    #[test]
    fn test_get_agents_in_namespace() {
        let conn = in_memory_conn();

        let mut input_a = basic_input("agent-ns1");
        input_a.namespaces = vec!["default".to_string(), "project-x".to_string()];
        register_agent(&conn, &input_a).expect("register a");

        let mut input_b = basic_input("agent-ns2");
        input_b.namespaces = vec!["project-x".to_string()];
        register_agent(&conn, &input_b).expect("register b");

        let mut input_c = basic_input("agent-ns3");
        input_c.namespaces = vec!["other".to_string()];
        register_agent(&conn, &input_c).expect("register c");

        let in_project_x = get_agents_in_namespace(&conn, "project-x").expect("query");
        let ids: Vec<&str> = in_project_x.iter().map(|a| a.agent_id.as_str()).collect();
        assert!(ids.contains(&"agent-ns1"), "agent-ns1 should be in project-x");
        assert!(ids.contains(&"agent-ns2"), "agent-ns2 should be in project-x");
        assert!(!ids.contains(&"agent-ns3"), "agent-ns3 should not be in project-x");

        let in_default = get_agents_in_namespace(&conn, "default").expect("query default");
        assert_eq!(in_default.len(), 1);
        assert_eq!(in_default[0].agent_id, "agent-ns1");
    }

    #[test]
    fn test_register_duplicate_updates() {
        let conn = in_memory_conn();
        register_agent(&conn, &basic_input("agent-dup")).expect("register first");

        let mut updated_input = basic_input("agent-dup");
        updated_input.display_name = "Updated Agent".to_string();
        updated_input.capabilities = vec!["admin".to_string()];
        let agent = register_agent(&conn, &updated_input).expect("register second (upsert)");

        assert_eq!(agent.display_name, "Updated Agent");
        assert_eq!(agent.capabilities, vec!["admin"]);
        assert_eq!(agent.status, "active");

        // Only one row should exist
        let all = list_agents(&conn, None).expect("list");
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_deregister_nonexistent() {
        let conn = in_memory_conn();

        let found = deregister_agent(&conn, "does-not-exist").expect("no db error");
        assert!(!found, "should return false for nonexistent agent");
    }

    #[test]
    fn test_heartbeat_nonexistent_returns_none() {
        let conn = in_memory_conn();

        let result = heartbeat_agent(&conn, "ghost-agent").expect("no db error");
        assert!(result.is_none(), "heartbeat on missing agent should return None");
    }

    #[test]
    fn test_register_empty_agent_id_fails() {
        let conn = in_memory_conn();
        let mut input = basic_input("");
        input.agent_id = "   ".to_string(); // blank

        let err = register_agent(&conn, &input);
        assert!(err.is_err(), "empty agent_id should fail");
    }
}
