//! Multi-User Authentication & Authorization (RML-886)
//!
//! Provides:
//! - User management with API keys
//! - Permission-based access control
//! - Memory ownership and sharing
//! - Namespace isolation

mod permissions;
mod tokens;
mod users;

pub use permissions::{Permission, PermissionSet, ResourceType};
pub use tokens::{ApiKey, ApiKeyManager, TokenClaims};
pub use users::{User, UserId, UserManager};

use crate::error::{EngramError, Result};
use rusqlite::Connection;

/// Authentication context for a request
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_id: UserId,
    pub permissions: PermissionSet,
    pub namespace: Option<String>,
}

impl AuthContext {
    /// Create a new auth context
    pub fn new(user_id: UserId, permissions: PermissionSet) -> Self {
        Self {
            user_id,
            permissions,
            namespace: None,
        }
    }

    /// Create auth context with namespace
    pub fn with_namespace(user_id: UserId, permissions: PermissionSet, namespace: String) -> Self {
        Self {
            user_id,
            permissions,
            namespace: Some(namespace),
        }
    }

    /// Check if user has permission
    pub fn has_permission(&self, permission: Permission, resource: ResourceType) -> bool {
        self.permissions.has_permission(permission, resource)
    }

    /// Require permission or return error
    pub fn require_permission(&self, permission: Permission, resource: ResourceType) -> Result<()> {
        if self.has_permission(permission, resource) {
            Ok(())
        } else {
            Err(EngramError::Unauthorized(format!(
                "Missing permission {:?} for {:?}",
                permission, resource
            )))
        }
    }

    /// Create a system-level context with full permissions
    pub fn system() -> Self {
        Self {
            user_id: UserId::system(),
            permissions: PermissionSet::admin(),
            namespace: None,
        }
    }

    /// Create an anonymous context with read-only public access
    pub fn anonymous() -> Self {
        Self {
            user_id: UserId::anonymous(),
            permissions: PermissionSet::read_only(),
            namespace: None,
        }
    }
}

/// Initialize auth tables in database
pub fn init_auth_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Users table
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            display_name TEXT,
            email TEXT,
            password_hash TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            is_admin INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- API keys table
        CREATE TABLE IF NOT EXISTS api_keys (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            key_hash TEXT UNIQUE NOT NULL,
            key_prefix TEXT NOT NULL,
            name TEXT NOT NULL,
            permissions TEXT NOT NULL DEFAULT '[]',
            namespace TEXT,
            expires_at TEXT,
            last_used_at TEXT,
            is_active INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- User namespaces (for multi-tenant isolation)
        CREATE TABLE IF NOT EXISTS namespaces (
            id TEXT PRIMARY KEY,
            name TEXT UNIQUE NOT NULL,
            owner_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            is_public INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Namespace memberships (shared access)
        CREATE TABLE IF NOT EXISTS namespace_members (
            namespace_id TEXT NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
            user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            role TEXT NOT NULL DEFAULT 'reader',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (namespace_id, user_id)
        );

        -- Memory ownership (links memories to users/namespaces)
        CREATE TABLE IF NOT EXISTS memory_ownership (
            memory_id TEXT NOT NULL,
            user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
            namespace_id TEXT REFERENCES namespaces(id) ON DELETE CASCADE,
            is_public INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (memory_id)
        );

        -- Indexes
        CREATE INDEX IF NOT EXISTS idx_api_keys_user ON api_keys(user_id);
        CREATE INDEX IF NOT EXISTS idx_api_keys_prefix ON api_keys(key_prefix);
        CREATE INDEX IF NOT EXISTS idx_namespace_members_user ON namespace_members(user_id);
        CREATE INDEX IF NOT EXISTS idx_memory_ownership_user ON memory_ownership(user_id);
        CREATE INDEX IF NOT EXISTS idx_memory_ownership_namespace ON memory_ownership(namespace_id);
        "#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_auth_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_auth_context_permissions() {
        let ctx = AuthContext::new(
            UserId::new(),
            PermissionSet::from_permissions(vec![
                (Permission::Read, ResourceType::Memory),
                (Permission::Write, ResourceType::Memory),
            ]),
        );

        assert!(ctx.has_permission(Permission::Read, ResourceType::Memory));
        assert!(ctx.has_permission(Permission::Write, ResourceType::Memory));
        assert!(!ctx.has_permission(Permission::Delete, ResourceType::Memory));
        assert!(!ctx.has_permission(Permission::Read, ResourceType::User));
    }

    #[test]
    fn test_system_context() {
        let ctx = AuthContext::system();
        assert!(ctx.has_permission(Permission::Admin, ResourceType::System));
        assert!(ctx.has_permission(Permission::Delete, ResourceType::Memory));
    }

    #[test]
    fn test_anonymous_context() {
        let ctx = AuthContext::anonymous();
        assert!(ctx.has_permission(Permission::Read, ResourceType::Memory));
        assert!(!ctx.has_permission(Permission::Write, ResourceType::Memory));
    }

    #[test]
    fn test_init_auth_tables() {
        let conn = setup_db();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%user%' OR name LIKE '%api%' OR name LIKE '%namespace%' OR name LIKE '%ownership%'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"users".to_string()));
        assert!(tables.contains(&"api_keys".to_string()));
        assert!(tables.contains(&"namespaces".to_string()));
    }
}
