//! User management

use crate::error::{EngramError, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// User identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(String);

impl UserId {
    /// Create a new random user ID
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Create from string
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// System user ID
    pub fn system() -> Self {
        Self("system".to_string())
    }

    /// Anonymous user ID
    pub fn anonymous() -> Self {
        Self("anonymous".to_string())
    }

    /// Get the string value
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// User record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub is_active: bool,
    pub is_admin: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    /// Create a new user
    pub fn new(username: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: UserId::new(),
            username: username.into(),
            display_name: None,
            email: None,
            is_active: true,
            is_admin: false,
            created_at: now,
            updated_at: now,
        }
    }

    /// Set display name
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Set email
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Set as admin
    pub fn with_admin(mut self, is_admin: bool) -> Self {
        self.is_admin = is_admin;
        self
    }
}

/// User management operations
pub struct UserManager<'a> {
    conn: &'a Connection,
}

impl<'a> UserManager<'a> {
    /// Create a new user manager
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create a new user
    pub fn create_user(&self, user: &User, password: Option<&str>) -> Result<()> {
        let password_hash = password.map(|p| hash_password(p));

        self.conn.execute(
            r#"
            INSERT INTO users (id, username, display_name, email, password_hash, is_active, is_admin, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                user.id.as_str(),
                user.username,
                user.display_name,
                user.email,
                password_hash,
                user.is_active,
                user.is_admin,
                user.created_at.to_rfc3339(),
                user.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Get user by ID
    pub fn get_user(&self, id: &UserId) -> Result<Option<User>> {
        self.conn
            .query_row(
                r#"
                SELECT id, username, display_name, email, is_active, is_admin, created_at, updated_at
                FROM users WHERE id = ?1
                "#,
                params![id.as_str()],
                |row| {
                    Ok(User {
                        id: UserId::from_string(row.get::<_, String>(0)?),
                        username: row.get(1)?,
                        display_name: row.get(2)?,
                        email: row.get(3)?,
                        is_active: row.get(4)?,
                        is_admin: row.get(5)?,
                        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .optional()
            .map_err(EngramError::from)
    }

    /// Get user by username
    pub fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        self.conn
            .query_row(
                r#"
                SELECT id, username, display_name, email, is_active, is_admin, created_at, updated_at
                FROM users WHERE username = ?1
                "#,
                params![username],
                |row| {
                    Ok(User {
                        id: UserId::from_string(row.get::<_, String>(0)?),
                        username: row.get(1)?,
                        display_name: row.get(2)?,
                        email: row.get(3)?,
                        is_active: row.get(4)?,
                        is_admin: row.get(5)?,
                        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                        updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .optional()
            .map_err(EngramError::from)
    }

    /// Verify user password
    pub fn verify_password(&self, username: &str, password: &str) -> Result<Option<User>> {
        let result: Option<(String, Option<String>)> = self
            .conn
            .query_row(
                "SELECT id, password_hash FROM users WHERE username = ?1 AND is_active = 1",
                params![username],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        if let Some((id, hash)) = result {
            if let Some(stored_hash) = hash {
                if verify_password(password, &stored_hash) {
                    return self.get_user(&UserId::from_string(id));
                }
            }
        }
        Ok(None)
    }

    /// Update user
    pub fn update_user(&self, user: &User) -> Result<()> {
        self.conn.execute(
            r#"
            UPDATE users SET
                username = ?2,
                display_name = ?3,
                email = ?4,
                is_active = ?5,
                is_admin = ?6,
                updated_at = ?7
            WHERE id = ?1
            "#,
            params![
                user.id.as_str(),
                user.username,
                user.display_name,
                user.email,
                user.is_active,
                user.is_admin,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Delete user
    pub fn delete_user(&self, id: &UserId) -> Result<bool> {
        let deleted = self
            .conn
            .execute("DELETE FROM users WHERE id = ?1", params![id.as_str()])?;
        Ok(deleted > 0)
    }

    /// List all users
    pub fn list_users(&self, include_inactive: bool) -> Result<Vec<User>> {
        let sql = if include_inactive {
            "SELECT id, username, display_name, email, is_active, is_admin, created_at, updated_at FROM users ORDER BY created_at DESC"
        } else {
            "SELECT id, username, display_name, email, is_active, is_admin, created_at, updated_at FROM users WHERE is_active = 1 ORDER BY created_at DESC"
        };

        let mut stmt = self.conn.prepare(sql)?;
        let users = stmt
            .query_map([], |row| {
                Ok(User {
                    id: UserId::from_string(row.get::<_, String>(0)?),
                    username: row.get(1)?,
                    display_name: row.get(2)?,
                    email: row.get(3)?,
                    is_active: row.get(4)?,
                    is_admin: row.get(5)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(users)
    }
}

/// Hash a password using SHA-256 (in production, use bcrypt or argon2)
fn hash_password(password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hex::encode(hasher.finalize())
}

/// Verify a password against a hash
fn verify_password(password: &str, hash: &str) -> bool {
    hash_password(password) == hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::init_auth_tables;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_auth_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_and_get_user() {
        let conn = setup_db();
        let manager = UserManager::new(&conn);

        let user = User::new("testuser")
            .with_display_name("Test User")
            .with_email("test@example.com");

        manager.create_user(&user, Some("password123")).unwrap();

        let fetched = manager.get_user(&user.id).unwrap().unwrap();
        assert_eq!(fetched.username, "testuser");
        assert_eq!(fetched.display_name, Some("Test User".to_string()));
        assert_eq!(fetched.email, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_get_user_by_username() {
        let conn = setup_db();
        let manager = UserManager::new(&conn);

        let user = User::new("findme");
        manager.create_user(&user, None).unwrap();

        let fetched = manager.get_user_by_username("findme").unwrap().unwrap();
        assert_eq!(fetched.id, user.id);
    }

    #[test]
    fn test_verify_password() {
        let conn = setup_db();
        let manager = UserManager::new(&conn);

        let user = User::new("authuser");
        manager.create_user(&user, Some("secret123")).unwrap();

        let verified = manager.verify_password("authuser", "secret123").unwrap();
        assert!(verified.is_some());

        let wrong = manager
            .verify_password("authuser", "wrongpassword")
            .unwrap();
        assert!(wrong.is_none());
    }

    #[test]
    fn test_update_user() {
        let conn = setup_db();
        let manager = UserManager::new(&conn);

        let mut user = User::new("updateme");
        manager.create_user(&user, None).unwrap();

        user.display_name = Some("Updated Name".to_string());
        manager.update_user(&user).unwrap();

        let fetched = manager.get_user(&user.id).unwrap().unwrap();
        assert_eq!(fetched.display_name, Some("Updated Name".to_string()));
    }

    #[test]
    fn test_delete_user() {
        let conn = setup_db();
        let manager = UserManager::new(&conn);

        let user = User::new("deleteme");
        manager.create_user(&user, None).unwrap();

        let deleted = manager.delete_user(&user.id).unwrap();
        assert!(deleted);

        let fetched = manager.get_user(&user.id).unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_list_users() {
        let conn = setup_db();
        let manager = UserManager::new(&conn);

        let user1 = User::new("user1");
        let mut user2 = User::new("user2");
        user2.is_active = false;

        manager.create_user(&user1, None).unwrap();
        manager.create_user(&user2, None).unwrap();

        let active = manager.list_users(false).unwrap();
        assert_eq!(active.len(), 1);

        let all = manager.list_users(true).unwrap();
        assert_eq!(all.len(), 2);
    }
}
