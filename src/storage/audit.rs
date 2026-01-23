//! Audit logging for all operations (RML-884)
//!
//! Append-only audit log for tracking who changed what and when.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::error::Result;
use crate::types::MemoryId;

/// Audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub user_id: Option<String>,
    pub action: AuditAction,
    pub memory_id: Option<MemoryId>,
    pub changes: Option<serde_json::Value>,
    pub ip_address: Option<String>,
}

/// Types of auditable actions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Create,
    Update,
    Delete,
    Link,
    Unlink,
    Search,
    Export,
    Import,
    SyncPush,
    SyncPull,
    Login,
    Logout,
}

impl AuditAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditAction::Create => "create",
            AuditAction::Update => "update",
            AuditAction::Delete => "delete",
            AuditAction::Link => "link",
            AuditAction::Unlink => "unlink",
            AuditAction::Search => "search",
            AuditAction::Export => "export",
            AuditAction::Import => "import",
            AuditAction::SyncPush => "sync_push",
            AuditAction::SyncPull => "sync_pull",
            AuditAction::Login => "login",
            AuditAction::Logout => "logout",
        }
    }
}

impl std::str::FromStr for AuditAction {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "create" => Ok(AuditAction::Create),
            "update" => Ok(AuditAction::Update),
            "delete" => Ok(AuditAction::Delete),
            "link" => Ok(AuditAction::Link),
            "unlink" => Ok(AuditAction::Unlink),
            "search" => Ok(AuditAction::Search),
            "export" => Ok(AuditAction::Export),
            "import" => Ok(AuditAction::Import),
            "sync_push" => Ok(AuditAction::SyncPush),
            "sync_pull" => Ok(AuditAction::SyncPull),
            "login" => Ok(AuditAction::Login),
            "logout" => Ok(AuditAction::Logout),
            _ => Err(format!("Unknown audit action: {}", s)),
        }
    }
}

/// Log an audit entry
pub fn log_audit(
    conn: &Connection,
    action: AuditAction,
    memory_id: Option<MemoryId>,
    user_id: Option<&str>,
    changes: Option<&serde_json::Value>,
    ip_address: Option<&str>,
) -> Result<i64> {
    let now = Utc::now().to_rfc3339();
    let changes_str = changes.map(|c| c.to_string());

    conn.execute(
        "INSERT INTO audit_log (timestamp, user_id, action, memory_id, changes, ip_address)
         VALUES (?, ?, ?, ?, ?, ?)",
        params![
            now,
            user_id,
            action.as_str(),
            memory_id,
            changes_str,
            ip_address,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// Calculate a diff between two memory states
pub fn calculate_diff(old: &serde_json::Value, new: &serde_json::Value) -> serde_json::Value {
    let mut diff = serde_json::Map::new();

    if let (Some(old_obj), Some(new_obj)) = (old.as_object(), new.as_object()) {
        // Check for changed/added fields
        for (key, new_val) in new_obj {
            match old_obj.get(key) {
                Some(old_val) if old_val != new_val => {
                    diff.insert(
                        key.clone(),
                        serde_json::json!({
                            "old": old_val,
                            "new": new_val,
                        }),
                    );
                }
                None => {
                    diff.insert(
                        key.clone(),
                        serde_json::json!({
                            "old": null,
                            "new": new_val,
                        }),
                    );
                }
                _ => {}
            }
        }

        // Check for removed fields
        for key in old_obj.keys() {
            if !new_obj.contains_key(key) {
                diff.insert(
                    key.clone(),
                    serde_json::json!({
                        "old": old_obj.get(key),
                        "new": null,
                    }),
                );
            }
        }
    }

    serde_json::Value::Object(diff)
}

/// Query audit log entries
pub fn query_audit_log(conn: &Connection, filter: &AuditFilter) -> Result<Vec<AuditEntry>> {
    let mut sql = String::from(
        "SELECT id, timestamp, user_id, action, memory_id, changes, ip_address
         FROM audit_log WHERE 1=1",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(memory_id) = filter.memory_id {
        sql.push_str(" AND memory_id = ?");
        params_vec.push(Box::new(memory_id));
    }

    if let Some(ref user_id) = filter.user_id {
        sql.push_str(" AND user_id = ?");
        params_vec.push(Box::new(user_id.clone()));
    }

    if let Some(ref action) = filter.action {
        sql.push_str(" AND action = ?");
        params_vec.push(Box::new(action.as_str().to_string()));
    }

    if let Some(ref since) = filter.since {
        sql.push_str(" AND timestamp >= ?");
        params_vec.push(Box::new(since.to_rfc3339()));
    }

    if let Some(ref until) = filter.until {
        sql.push_str(" AND timestamp <= ?");
        params_vec.push(Box::new(until.to_rfc3339()));
    }

    sql.push_str(" ORDER BY timestamp DESC");

    if let Some(limit) = filter.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }

    let params_ref: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;

    let entries: Vec<AuditEntry> = stmt
        .query_map(params_ref.as_slice(), |row| {
            let timestamp_str: String = row.get("timestamp")?;
            let action_str: String = row.get("action")?;
            let changes_str: Option<String> = row.get("changes")?;

            Ok(AuditEntry {
                id: row.get("id")?,
                timestamp: DateTime::parse_from_rfc3339(&timestamp_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                user_id: row.get("user_id")?,
                action: action_str.parse().unwrap_or(AuditAction::Update),
                memory_id: row.get("memory_id")?,
                changes: changes_str.and_then(|s| serde_json::from_str(&s).ok()),
                ip_address: row.get("ip_address")?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(entries)
}

/// Filter for querying audit log
#[derive(Debug, Clone, Default)]
pub struct AuditFilter {
    pub memory_id: Option<MemoryId>,
    pub user_id: Option<String>,
    pub action: Option<AuditAction>,
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub limit: Option<i64>,
}

/// Get audit summary for a memory
pub fn get_memory_audit_summary(conn: &Connection, memory_id: MemoryId) -> Result<AuditSummary> {
    let filter = AuditFilter {
        memory_id: Some(memory_id),
        limit: Some(1000),
        ..Default::default()
    };

    let entries = query_audit_log(conn, &filter)?;

    let total_changes = entries.len();
    let unique_users: std::collections::HashSet<_> =
        entries.iter().filter_map(|e| e.user_id.as_ref()).collect();
    let first_action = entries.last().map(|e| e.timestamp);
    let last_action = entries.first().map(|e| e.timestamp);

    let mut action_counts: HashMap<String, i64> = HashMap::new();
    for entry in &entries {
        *action_counts
            .entry(entry.action.as_str().to_string())
            .or_insert(0) += 1;
    }

    Ok(AuditSummary {
        memory_id,
        total_changes,
        unique_users: unique_users.len(),
        first_action,
        last_action,
        action_counts,
    })
}

/// Summary of audit activity for a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    pub memory_id: MemoryId,
    pub total_changes: usize,
    pub unique_users: usize,
    pub first_action: Option<DateTime<Utc>>,
    pub last_action: Option<DateTime<Utc>>,
    pub action_counts: HashMap<String, i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_diff() {
        let old = serde_json::json!({
            "content": "old content",
            "importance": 0.5,
            "removed_field": "value"
        });

        let new = serde_json::json!({
            "content": "new content",
            "importance": 0.5,
            "new_field": "new value"
        });

        let diff = calculate_diff(&old, &new);
        let diff_obj = diff.as_object().unwrap();

        assert!(diff_obj.contains_key("content"));
        assert!(diff_obj.contains_key("removed_field"));
        assert!(diff_obj.contains_key("new_field"));
        assert!(!diff_obj.contains_key("importance")); // unchanged
    }

    #[test]
    fn test_audit_action_roundtrip() {
        for action in [
            AuditAction::Create,
            AuditAction::Update,
            AuditAction::Delete,
        ] {
            let s = action.as_str();
            let parsed: AuditAction = s.parse().unwrap();
            assert_eq!(action, parsed);
        }
    }
}
