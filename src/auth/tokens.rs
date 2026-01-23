//! API key and token management

use crate::auth::{PermissionSet, UserId};
use crate::error::{EngramError, Result};
use chrono::{DateTime, Utc};
use rand::Rng;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// API key with prefix for easy identification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub user_id: UserId,
    pub name: String,
    pub key_prefix: String,
    pub permissions: PermissionSet,
    pub namespace: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// Token claims for validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    pub user_id: UserId,
    pub key_id: String,
    pub permissions: PermissionSet,
    pub namespace: Option<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl TokenClaims {
    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            return Utc::now() > exp;
        }
        false
    }
}

/// API key manager
pub struct ApiKeyManager<'a> {
    conn: &'a Connection,
}

impl<'a> ApiKeyManager<'a> {
    /// Create a new API key manager
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Generate a new API key
    /// Returns (ApiKey, raw_key) - raw_key should only be shown once
    pub fn create_api_key(
        &self,
        user_id: &UserId,
        name: &str,
        permissions: PermissionSet,
        namespace: Option<String>,
        expires_in_days: Option<i64>,
    ) -> Result<(ApiKey, String)> {
        let id = Uuid::new_v4().to_string();
        let raw_key = generate_api_key();
        let key_hash = hash_key(&raw_key);
        let key_prefix = &raw_key[..12]; // Show first 12 chars for identification

        let expires_at = expires_in_days.map(|days| Utc::now() + chrono::Duration::days(days));

        let permissions_json = serde_json::to_string(&permissions)?;

        self.conn.execute(
            r#"
            INSERT INTO api_keys (id, user_id, key_hash, key_prefix, name, permissions, namespace, expires_at, is_active, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, datetime('now'))
            "#,
            params![
                id,
                user_id.as_str(),
                key_hash,
                key_prefix,
                name,
                permissions_json,
                namespace,
                expires_at.map(|dt| dt.to_rfc3339()),
            ],
        )?;

        let api_key = ApiKey {
            id,
            user_id: user_id.clone(),
            name: name.to_string(),
            key_prefix: key_prefix.to_string(),
            permissions,
            namespace,
            expires_at,
            last_used_at: None,
            is_active: true,
            created_at: Utc::now(),
        };

        Ok((api_key, raw_key))
    }

    /// Validate an API key and return claims
    pub fn validate_key(&self, raw_key: &str) -> Result<Option<TokenClaims>> {
        let key_hash = hash_key(raw_key);

        let result: Option<(String, String, String, Option<String>, Option<String>, bool)> = self
            .conn
            .query_row(
                r#"
                SELECT ak.id, ak.user_id, ak.permissions, ak.namespace, ak.expires_at, u.is_active as user_active
                FROM api_keys ak
                JOIN users u ON ak.user_id = u.id
                WHERE ak.key_hash = ?1 AND ak.is_active = 1
                "#,
                params![key_hash],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
            )
            .optional()?;

        if let Some((key_id, user_id, permissions_json, namespace, expires_at_str, user_active)) =
            result
        {
            if !user_active {
                return Ok(None);
            }

            let expires_at = expires_at_str
                .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                .map(|dt| dt.with_timezone(&Utc));

            // Check expiration
            if let Some(exp) = expires_at {
                if Utc::now() > exp {
                    return Ok(None);
                }
            }

            // Update last used
            self.conn.execute(
                "UPDATE api_keys SET last_used_at = datetime('now') WHERE id = ?1",
                params![key_id],
            )?;

            let permissions: PermissionSet = serde_json::from_str(&permissions_json)?;

            Ok(Some(TokenClaims {
                user_id: UserId::from_string(user_id),
                key_id,
                permissions,
                namespace,
                issued_at: Utc::now(),
                expires_at,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get API key by ID (without the raw key)
    pub fn get_key(&self, id: &str) -> Result<Option<ApiKey>> {
        self.conn
            .query_row(
                r#"
                SELECT id, user_id, key_prefix, name, permissions, namespace, expires_at, last_used_at, is_active, created_at
                FROM api_keys WHERE id = ?1
                "#,
                params![id],
                |row| {
                    let permissions_json: String = row.get(4)?;
                    Ok(ApiKey {
                        id: row.get(0)?,
                        user_id: UserId::from_string(row.get::<_, String>(1)?),
                        key_prefix: row.get(2)?,
                        name: row.get(3)?,
                        permissions: serde_json::from_str(&permissions_json).unwrap_or_default(),
                        namespace: row.get(5)?,
                        expires_at: row.get::<_, Option<String>>(6)?
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        last_used_at: row.get::<_, Option<String>>(7)?
                            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                            .map(|dt| dt.with_timezone(&Utc)),
                        is_active: row.get(8)?,
                        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                    })
                },
            )
            .optional()
            .map_err(EngramError::from)
    }

    /// List API keys for a user
    pub fn list_keys(&self, user_id: &UserId) -> Result<Vec<ApiKey>> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, user_id, key_prefix, name, permissions, namespace, expires_at, last_used_at, is_active, created_at
            FROM api_keys WHERE user_id = ?1 ORDER BY created_at DESC
            "#,
        )?;

        let keys = stmt
            .query_map(params![user_id.as_str()], |row| {
                let permissions_json: String = row.get(4)?;
                Ok(ApiKey {
                    id: row.get(0)?,
                    user_id: UserId::from_string(row.get::<_, String>(1)?),
                    key_prefix: row.get(2)?,
                    name: row.get(3)?,
                    permissions: serde_json::from_str(&permissions_json).unwrap_or_default(),
                    namespace: row.get(5)?,
                    expires_at: row
                        .get::<_, Option<String>>(6)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    last_used_at: row
                        .get::<_, Option<String>>(7)?
                        .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                        .map(|dt| dt.with_timezone(&Utc)),
                    is_active: row.get(8)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(keys)
    }

    /// Revoke an API key
    pub fn revoke_key(&self, id: &str) -> Result<bool> {
        let updated = self.conn.execute(
            "UPDATE api_keys SET is_active = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(updated > 0)
    }

    /// Delete an API key
    pub fn delete_key(&self, id: &str) -> Result<bool> {
        let deleted = self
            .conn
            .execute("DELETE FROM api_keys WHERE id = ?1", params![id])?;
        Ok(deleted > 0)
    }
}

/// Generate a secure API key
fn generate_api_key() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    format!("eng_{}", hex::encode(bytes))
}

/// Hash an API key for storage
fn hash_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{init_auth_tables, Permission, ResourceType, User, UserManager};

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_auth_tables(&conn).unwrap();
        conn
    }

    #[test]
    fn test_create_and_validate_api_key() {
        let conn = setup_db();

        // Create user first
        let user = User::new("testuser");
        UserManager::new(&conn).create_user(&user, None).unwrap();

        // Create API key
        let manager = ApiKeyManager::new(&conn);
        let (api_key, raw_key) = manager
            .create_api_key(
                &user.id,
                "Test Key",
                PermissionSet::standard_user(),
                None,
                None,
            )
            .unwrap();

        assert!(raw_key.starts_with("eng_"));
        assert_eq!(api_key.name, "Test Key");

        // Validate key
        let claims = manager.validate_key(&raw_key).unwrap().unwrap();
        assert_eq!(claims.user_id, user.id);
        assert!(claims
            .permissions
            .has_permission(Permission::Read, ResourceType::Memory));
    }

    #[test]
    fn test_validate_invalid_key() {
        let conn = setup_db();
        let manager = ApiKeyManager::new(&conn);

        let claims = manager.validate_key("eng_invalid_key_here").unwrap();
        assert!(claims.is_none());
    }

    #[test]
    fn test_revoke_key() {
        let conn = setup_db();

        let user = User::new("testuser");
        UserManager::new(&conn).create_user(&user, None).unwrap();

        let manager = ApiKeyManager::new(&conn);
        let (api_key, raw_key) = manager
            .create_api_key(
                &user.id,
                "Revoke Test",
                PermissionSet::read_only(),
                None,
                None,
            )
            .unwrap();

        // Key should work
        assert!(manager.validate_key(&raw_key).unwrap().is_some());

        // Revoke key
        manager.revoke_key(&api_key.id).unwrap();

        // Key should no longer work
        assert!(manager.validate_key(&raw_key).unwrap().is_none());
    }

    #[test]
    fn test_expired_key() {
        let conn = setup_db();

        let user = User::new("testuser");
        UserManager::new(&conn).create_user(&user, None).unwrap();

        let manager = ApiKeyManager::new(&conn);

        // Create key that expires in -1 days (already expired)
        // We'll manually set the expiration to test
        let (_api_key, raw_key) = manager
            .create_api_key(
                &user.id,
                "Expiring Key",
                PermissionSet::read_only(),
                None,
                Some(-1),
            )
            .unwrap();

        // Key should be expired
        let claims = manager.validate_key(&raw_key).unwrap();
        assert!(claims.is_none());
    }

    #[test]
    fn test_list_keys() {
        let conn = setup_db();

        let user = User::new("testuser");
        UserManager::new(&conn).create_user(&user, None).unwrap();

        let manager = ApiKeyManager::new(&conn);
        manager
            .create_api_key(&user.id, "Key 1", PermissionSet::read_only(), None, None)
            .unwrap();
        manager
            .create_api_key(
                &user.id,
                "Key 2",
                PermissionSet::standard_user(),
                None,
                None,
            )
            .unwrap();

        let keys = manager.list_keys(&user.id).unwrap();
        assert_eq!(keys.len(), 2);
    }
}
