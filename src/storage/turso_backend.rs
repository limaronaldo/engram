//! Turso/libSQL implementation of the StorageBackend trait (Phase 6 - ENG-54)
//!
//! This module provides a Turso/libSQL-based storage backend that implements
//! the `StorageBackend` trait, enabling distributed SQLite with edge replicas.
//!
//! # Features
//!
//! - **Embedded replicas**: Local SQLite with sync to Turso cloud
//! - **Edge-native**: Sub-millisecond reads from local replica
//! - **Sync on demand**: Push/pull changes to cloud
//! - **Compatible schema**: Same migrations as SQLite backend
//!
//! # Usage
//!
//! ```rust,ignore
//! use engram::storage::TursoBackend;
//!
//! // Connect to Turso cloud with embedded replica
//! let backend = TursoBackend::new(
//!     "libsql://your-db.turso.io",
//!     "your-auth-token",
//!     Some("/path/to/local/replica.db"),
//! ).await?;
//!
//! // Or use local-only mode (no cloud sync)
//! let backend = TursoBackend::local_only("/path/to/db.sqlite").await?;
//! ```

#![cfg(feature = "turso")]

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Utc};
use libsql::{Builder, Connection, Database};
use tokio::sync::RwLock;

use crate::error::{EngramError, Result};
use crate::storage::migrations::SCHEMA_VERSION;
use crate::storage::queries::compute_content_hash;
use crate::types::{
    normalize_workspace, CreateMemoryInput, CrossReference, EdgeType, LifecycleState, ListOptions,
    MatchInfo, Memory, MemoryId, MemoryScope, MemoryTier, MemoryType, RelationSource,
    SearchOptions, SearchResult, SearchStrategy, SortField, SortOrder, UpdateMemoryInput,
    Visibility,
};

use super::backend::{
    BatchCreateResult, BatchDeleteResult, CloudSyncBackend, HealthStatus, StorageBackend,
    StorageStats, SyncDelta, SyncResult, SyncState, TransactionalBackend,
};

const MEMORY_COLUMNS: &str = "id, content, memory_type, importance, access_count, created_at, updated_at, last_accessed_at, owner_id, visibility, version, has_embedding, metadata, scope_type, scope_id, workspace, tier, expires_at, content_hash, event_time, event_duration_seconds, trigger_pattern, procedure_success_count, procedure_failure_count, summary_of_id, lifecycle_state";

/// Turso/libSQL storage backend configuration
#[derive(Debug, Clone)]
pub struct TursoConfig {
    /// Turso database URL (e.g., "libsql://your-db.turso.io")
    pub url: String,
    /// Authentication token for Turso cloud
    pub auth_token: Option<String>,
    /// Path to local embedded replica (for offline support)
    pub local_replica_path: Option<String>,
    /// Sync interval in seconds (0 = manual sync only)
    pub sync_interval_secs: u64,
    /// Whether to sync on startup
    pub sync_on_startup: bool,
}

impl Default for TursoConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            auth_token: None,
            local_replica_path: None,
            sync_interval_secs: 60,
            sync_on_startup: true,
        }
    }
}

/// Turso/libSQL-based storage backend
///
/// Implements the `StorageBackend` trait using libSQL (Turso's fork of SQLite)
/// with support for embedded replicas and cloud sync.
pub struct TursoBackend {
    db: Database,
    conn: Arc<RwLock<Connection>>,
    config: TursoConfig,
    schema_initialized: bool,
}

impl TursoBackend {
    /// Create a new Turso backend connected to Turso cloud
    ///
    /// # Arguments
    /// * `url` - Turso database URL
    /// * `auth_token` - Authentication token
    /// * `local_replica_path` - Optional path for embedded replica
    pub async fn new(
        url: &str,
        auth_token: &str,
        local_replica_path: Option<&str>,
    ) -> Result<Self> {
        let config = TursoConfig {
            url: url.to_string(),
            auth_token: Some(auth_token.to_string()),
            local_replica_path: local_replica_path.map(|s| s.to_string()),
            ..Default::default()
        };

        Self::with_config(config).await
    }

    /// Create a new Turso backend with custom configuration
    pub async fn with_config(config: TursoConfig) -> Result<Self> {
        let db = if let Some(ref replica_path) = config.local_replica_path {
            // Embedded replica mode: local SQLite with sync to cloud
            Builder::new_remote_replica(
                replica_path,
                config.url.clone(),
                config.auth_token.clone().unwrap_or_default(),
            )
            .build()
            .await
            .map_err(|e| EngramError::Storage(format!("Failed to create Turso replica: {}", e)))?
        } else if config.url.starts_with("libsql://") || config.url.starts_with("https://") {
            // Remote-only mode: direct connection to Turso cloud
            Builder::new_remote(
                config.url.clone(),
                config.auth_token.clone().unwrap_or_default(),
            )
            .build()
            .await
            .map_err(|e| EngramError::Storage(format!("Failed to connect to Turso: {}", e)))?
        } else {
            // Local-only mode: pure SQLite via libSQL
            Builder::new_local(&config.url).build().await.map_err(|e| {
                EngramError::Storage(format!("Failed to open local database: {}", e))
            })?
        };

        let conn = db
            .connect()
            .map_err(|e| EngramError::Storage(format!("Failed to get connection: {}", e)))?;

        let mut backend = Self {
            db,
            conn: Arc::new(RwLock::new(conn)),
            config,
            schema_initialized: false,
        };

        // Initialize schema
        backend.init_schema().await?;

        // Sync on startup if configured
        if backend.config.sync_on_startup && backend.config.local_replica_path.is_some() {
            let _ = backend.sync().await;
        }

        Ok(backend)
    }

    /// Create a local-only Turso backend (no cloud sync)
    pub async fn local_only(path: &str) -> Result<Self> {
        let config = TursoConfig {
            url: path.to_string(),
            auth_token: None,
            local_replica_path: None,
            sync_interval_secs: 0,
            sync_on_startup: false,
        };
        Self::with_config(config).await
    }

    /// Create an in-memory Turso backend (useful for testing)
    pub async fn in_memory() -> Result<Self> {
        Self::local_only(":memory:").await
    }

    /// Initialize the database schema
    async fn init_schema(&mut self) -> Result<()> {
        if self.schema_initialized {
            return Ok(());
        }

        let conn = self.conn.write().await;

        // Create schema version table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schema_version (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            (),
        )
        .await
        .map_err(|e| EngramError::Storage(format!("Schema init failed: {}", e)))?;

        // Check current version
        let version: i32 = conn
            .query("SELECT COALESCE(MAX(version), 0) FROM schema_version", ())
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?
            .next()
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?
            .map(|row| row.get::<i32>(0).unwrap_or(0))
            .unwrap_or(0);

        // Apply migrations
        if version < SCHEMA_VERSION {
            self.apply_migration_v1(&conn).await?;
        }

        self.schema_initialized = true;
        Ok(())
    }

    /// Apply migration v1 - base schema
    async fn apply_migration_v1(&self, conn: &Connection) -> Result<()> {
        // Memories table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                memory_type TEXT NOT NULL DEFAULT 'note',
                importance REAL NOT NULL DEFAULT 0.5,
                access_count INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_accessed_at TEXT,
                owner_id TEXT,
                visibility TEXT NOT NULL DEFAULT 'private',
                version INTEGER NOT NULL DEFAULT 1,
                has_embedding INTEGER NOT NULL DEFAULT 0,
                embedding_queued_at TEXT,
                valid_from TEXT NOT NULL DEFAULT (datetime('now')),
                valid_to TEXT,
                metadata TEXT NOT NULL DEFAULT '{}',
                scope_type TEXT NOT NULL DEFAULT 'global',
                scope_id TEXT,
                expires_at TEXT,
                content_hash TEXT,
                workspace TEXT NOT NULL DEFAULT 'default',
                tier TEXT NOT NULL DEFAULT 'permanent',
                event_time TEXT,
                event_duration_seconds INTEGER,
                trigger_pattern TEXT,
                procedure_success_count INTEGER DEFAULT 0,
                procedure_failure_count INTEGER DEFAULT 0,
                summary_of_id INTEGER REFERENCES memories(id) ON DELETE SET NULL,
                lifecycle_state TEXT DEFAULT 'active'
            )",
            (),
        )
        .await
        .map_err(|e| EngramError::Storage(e.to_string()))?;

        // Tags table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            (),
        )
        .await
        .map_err(|e| EngramError::Storage(e.to_string()))?;

        // Memory-Tags junction table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_tags (
                memory_id INTEGER NOT NULL,
                tag_id INTEGER NOT NULL,
                PRIMARY KEY (memory_id, tag_id),
                FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
                FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
            )",
            (),
        )
        .await
        .map_err(|e| EngramError::Storage(e.to_string()))?;

        // Cross-references table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS crossrefs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                from_id INTEGER NOT NULL,
                to_id INTEGER NOT NULL,
                edge_type TEXT NOT NULL DEFAULT 'related_to',
                score REAL NOT NULL,
                confidence REAL NOT NULL DEFAULT 1.0,
                strength REAL NOT NULL DEFAULT 1.0,
                source TEXT NOT NULL DEFAULT 'auto',
                source_context TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                valid_from TEXT NOT NULL DEFAULT (datetime('now')),
                valid_to TEXT,
                pinned INTEGER NOT NULL DEFAULT 0,
                metadata TEXT NOT NULL DEFAULT '{}',
                FOREIGN KEY (from_id) REFERENCES memories(id) ON DELETE CASCADE,
                FOREIGN KEY (to_id) REFERENCES memories(id) ON DELETE CASCADE,
                UNIQUE(from_id, to_id, edge_type)
            )",
            (),
        )
        .await
        .map_err(|e| EngramError::Storage(e.to_string()))?;

        // Identities table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS identities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                canonical_id TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                identity_type TEXT DEFAULT 'unknown',
                metadata TEXT DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            (),
        )
        .await
        .map_err(|e| EngramError::Storage(e.to_string()))?;

        // Entities table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS entities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                metadata TEXT DEFAULT '{}',
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                UNIQUE(name, entity_type)
            )",
            (),
        )
        .await
        .map_err(|e| EngramError::Storage(e.to_string()))?;

        // Create indexes
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_workspace ON memories(workspace)",
            (),
        )
        .await
        .ok();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_type ON memories(memory_type)",
            (),
        )
        .await
        .ok();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_tier ON memories(tier)",
            (),
        )
        .await
        .ok();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_lifecycle ON memories(lifecycle_state)",
            (),
        )
        .await
        .ok();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_memories_created ON memories(created_at)",
            (),
        )
        .await
        .ok();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_crossrefs_from ON crossrefs(from_id)",
            (),
        )
        .await
        .ok();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_crossrefs_to ON crossrefs(to_id)",
            (),
        )
        .await
        .ok();

        // Record migration
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?)",
            libsql::params![SCHEMA_VERSION],
        )
        .await
        .map_err(|e| EngramError::Storage(e.to_string()))?;

        Ok(())
    }

    /// Sync with Turso cloud (if using embedded replica)
    pub async fn sync(&self) -> Result<SyncResult> {
        if self.config.local_replica_path.is_none() {
            return Ok(SyncResult {
                success: true,
                pushed_count: 0,
                pulled_count: 0,
                conflicts_resolved: 0,
                error: Some("No local replica configured".to_string()),
                new_version: 0,
            });
        }

        self.db
            .sync()
            .await
            .map_err(|e| EngramError::Sync(format!("Turso sync failed: {}", e)))?;

        Ok(SyncResult {
            success: true,
            pushed_count: 0,
            pulled_count: 0,
            conflicts_resolved: 0,
            error: None,
            new_version: 0,
        })
    }

    /// Execute a query and return results
    async fn query_memories(&self, sql: &str, params: Vec<libsql::Value>) -> Result<Vec<Memory>> {
        let conn = self.conn.read().await;
        let mut stmt = conn
            .prepare(sql)
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?;

        let rows = stmt
            .query(params)
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?;

        let mut memories = Vec::new();
        let mut rows = rows;

        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?
        {
            let mut memory = self.row_to_memory(&row)?;
            memory.tags = self.load_tags_with_conn(&conn, memory.id).await?;
            memories.push(memory);
        }

        Ok(memories)
    }

    /// Convert a database row to a Memory struct
    fn row_to_memory(&self, row: &libsql::Row) -> Result<Memory> {
        let id: i64 = row
            .get(0)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let content: String = row
            .get(1)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let memory_type_str: String = row
            .get(2)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let importance: f32 = row
            .get::<f64>(3)
            .map_err(|e| EngramError::Storage(e.to_string()))? as f32;
        let access_count: i32 =
            row.get::<i64>(4)
                .map_err(|e| EngramError::Storage(e.to_string()))? as i32;
        let created_at: String = row
            .get(5)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let updated_at: String = row
            .get(6)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let last_accessed_at: Option<String> = row
            .get(7)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let owner_id: Option<String> = row
            .get(8)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let visibility_str: String = row
            .get(9)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let version: i32 = row
            .get::<i64>(10)
            .map_err(|e| EngramError::Storage(e.to_string()))? as i32;
        let has_embedding: i32 =
            row.get::<i64>(11)
                .map_err(|e| EngramError::Storage(e.to_string()))? as i32;
        let metadata_str: String = row
            .get(12)
            .map_err(|e| EngramError::Storage(e.to_string()))?;
        let scope_type: String = row.get(13).unwrap_or_else(|_| "global".to_string());
        let scope_id: Option<String> = row.get(14).unwrap_or(None);
        let workspace: String = row.get(15).unwrap_or_else(|_| "default".to_string());
        let tier_str: String = row.get(16).unwrap_or_else(|_| "permanent".to_string());
        let expires_at: Option<String> = row.get(17).unwrap_or(None);
        let content_hash: Option<String> = row.get(18).unwrap_or(None);
        let event_time: Option<String> = row.get(19).unwrap_or(None);
        let event_duration_seconds: Option<i64> = row.get(20).unwrap_or(None);
        let trigger_pattern: Option<String> = row.get(21).unwrap_or(None);
        let procedure_success_count: i32 = row.get(22).unwrap_or(0);
        let procedure_failure_count: i32 = row.get(23).unwrap_or(0);
        let summary_of_id: Option<i64> = row.get(24).unwrap_or(None);
        let lifecycle_state_str: Option<String> = row.get(25).unwrap_or(None);

        let memory_type = memory_type_str.parse().unwrap_or(MemoryType::Note);
        let visibility = match visibility_str.as_str() {
            "shared" => Visibility::Shared,
            "public" => Visibility::Public,
            _ => Visibility::Private,
        };

        let scope = match (scope_type.as_str(), scope_id) {
            ("user", Some(id)) => MemoryScope::User { user_id: id },
            ("session", Some(id)) => MemoryScope::Session { session_id: id },
            ("agent", Some(id)) => MemoryScope::Agent { agent_id: id },
            _ => MemoryScope::Global,
        };

        let metadata: HashMap<String, serde_json::Value> =
            serde_json::from_str(&metadata_str).unwrap_or_default();
        let tier = tier_str.parse().unwrap_or(MemoryTier::Permanent);
        let lifecycle_state = lifecycle_state_str
            .and_then(|s| s.parse().ok())
            .unwrap_or(LifecycleState::Active);

        Ok(Memory {
            id,
            content,
            memory_type,
            tags: Vec::new(),
            metadata,
            importance,
            access_count,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            updated_at: DateTime::parse_from_rfc3339(&updated_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now()),
            last_accessed_at: Self::parse_datetime(last_accessed_at),
            owner_id,
            visibility,
            scope,
            workspace,
            tier,
            version,
            has_embedding: has_embedding != 0,
            expires_at: Self::parse_datetime(expires_at),
            content_hash,
            event_time: Self::parse_datetime(event_time),
            event_duration_seconds,
            trigger_pattern,
            procedure_success_count,
            procedure_failure_count,
            summary_of_id,
            lifecycle_state,
        })
    }

    fn parse_datetime(value: Option<String>) -> Option<DateTime<Utc>> {
        value.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        })
    }

    async fn load_tags_with_conn(
        &self,
        conn: &Connection,
        memory_id: MemoryId,
    ) -> Result<Vec<String>> {
        let mut stmt = conn
            .prepare(
                "SELECT t.name
                 FROM tags t
                 INNER JOIN memory_tags mt ON mt.tag_id = t.id
                 WHERE mt.memory_id = ?
                 ORDER BY t.name",
            )
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?;

        let rows = stmt
            .query(libsql::params![memory_id])
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?;

        let mut tags = Vec::new();
        let mut rows = rows;
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?
        {
            let name: String = row.get(0).unwrap_or_default();
            tags.push(name);
        }

        Ok(tags)
    }
}

impl StorageBackend for TursoBackend {
    fn create_memory(&self, input: CreateMemoryInput) -> Result<Memory> {
        // Use tokio runtime to run async code in sync context
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;

            let now = Utc::now();
            let now_str = now.to_rfc3339();
            let importance = input.importance.unwrap_or(0.5);

            let workspace = normalize_workspace(input.workspace.as_deref().unwrap_or("default"))
                .map_err(|e| EngramError::InvalidInput(e.to_string()))?;

            let metadata_json = serde_json::to_string(&input.metadata)?;
            let scope_type = input.scope.scope_type();
            let scope_id = input.scope.scope_id().map(|s| s.to_string());
            let tier = input.tier;

            let expires_at = match tier {
                MemoryTier::Permanent => {
                    if input.ttl_seconds.is_some() && input.ttl_seconds != Some(0) {
                        return Err(EngramError::InvalidInput(
                            "Permanent tier memories cannot have a TTL. Use Daily tier for expiring memories.".to_string(),
                        ));
                    }
                    None
                }
                MemoryTier::Daily => {
                    let ttl = input.ttl_seconds.filter(|&t| t > 0).unwrap_or(86400);
                    Some((now + chrono::Duration::seconds(ttl)).to_rfc3339())
                }
            };

            let content_hash = compute_content_hash(&input.content);
            let event_time = input.event_time.map(|dt| dt.to_rfc3339());

            conn.execute(
                "INSERT INTO memories (
                    content, memory_type, importance, metadata, created_at, updated_at, valid_from,
                    scope_type, scope_id, workspace, tier, expires_at, content_hash,
                    event_time, event_duration_seconds, trigger_pattern, summary_of_id, lifecycle_state
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                libsql::params![
                    input.content.clone(),
                    input.memory_type.as_str(),
                    importance as f64,
                    metadata_json,
                    now_str.clone(),
                    now_str.clone(),
                    now_str,
                    scope_type,
                    scope_id,
                    workspace,
                    tier.as_str(),
                    expires_at,
                    content_hash,
                    event_time,
                    input.event_duration_seconds,
                    input.trigger_pattern.clone(),
                    input.summary_of_id,
                    LifecycleState::Active.to_string(),
                ],
            )
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?;

            let id = conn.last_insert_rowid();

            // Insert tags
            for tag in &input.tags {
                // Ensure tag exists
                conn.execute(
                    "INSERT OR IGNORE INTO tags (name) VALUES (?)",
                    libsql::params![tag.clone()],
                ).await.ok();

                // Link tag to memory
                conn.execute(
                    "INSERT OR IGNORE INTO memory_tags (memory_id, tag_id)
                     SELECT ?, id FROM tags WHERE name = ?",
                    libsql::params![id, tag.clone()],
                ).await.ok();
            }

            drop(conn);

            let sql = format!(
                "SELECT {} FROM memories WHERE id = ? AND valid_to IS NULL",
                MEMORY_COLUMNS
            );
            let mut memories = self
                .query_memories(&sql, vec![libsql::Value::Integer(id)])
                .await?;

            memories
                .pop()
                .ok_or_else(|| EngramError::NotFound(id))
        })
    }

    fn create_memories_batch(&self, inputs: Vec<CreateMemoryInput>) -> Result<BatchCreateResult> {
        let start = Instant::now();
        let mut created = Vec::new();
        let mut failed = Vec::new();

        for (idx, input) in inputs.into_iter().enumerate() {
            match self.create_memory(input) {
                Ok(memory) => created.push(memory),
                Err(e) => failed.push((idx, e.to_string())),
            }
        }

        Ok(BatchCreateResult {
            created,
            failed,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn get_memory(&self, id: MemoryId) -> Result<Option<Memory>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let sql = format!(
                "SELECT {} FROM memories WHERE id = ? AND valid_to IS NULL",
                MEMORY_COLUMNS
            );
            let memories = self
                .query_memories(&sql, vec![libsql::Value::Integer(id)])
                .await?;

            Ok(memories.into_iter().next())
        })
    }

    fn delete_memories_batch(&self, ids: Vec<MemoryId>) -> Result<BatchDeleteResult> {
        let mut deleted_count = 0;
        let mut not_found = Vec::new();
        let mut failed = Vec::new();

        for id in ids {
            match self.delete_memory(id) {
                Ok(()) => deleted_count += 1,
                Err(EngramError::NotFound(_)) => not_found.push(id),
                Err(e) => failed.push((id, e.to_string())),
            }
        }

        Ok(BatchDeleteResult {
            deleted_count,
            not_found,
            failed,
        })
    }

    fn update_memory(&self, id: MemoryId, input: UpdateMemoryInput) -> Result<Memory> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            let now = Utc::now().to_rfc3339();

            let mut updates = vec!["updated_at = ?".to_string()];
            let mut params: Vec<libsql::Value> = vec![libsql::Value::Text(now)];

            if let Some(ref content) = input.content {
                updates.push("content = ?".to_string());
                params.push(libsql::Value::Text(content.clone()));
                let new_hash = compute_content_hash(content);
                updates.push("content_hash = ?".to_string());
                params.push(libsql::Value::Text(new_hash));
            }

            if let Some(ref memory_type) = input.memory_type {
                updates.push("memory_type = ?".to_string());
                params.push(libsql::Value::Text(memory_type.as_str().to_string()));
            }

            if let Some(importance) = input.importance {
                updates.push("importance = ?".to_string());
                params.push(libsql::Value::Real(importance as f64));
            }

            if let Some(ref metadata) = input.metadata {
                let metadata_json =
                    serde_json::to_string(metadata).map_err(EngramError::Serialization)?;
                updates.push("metadata = ?".to_string());
                params.push(libsql::Value::Text(metadata_json));
            }

            if let Some(ref scope) = input.scope {
                updates.push("scope_type = ?".to_string());
                params.push(libsql::Value::Text(scope.scope_type().to_string()));
                updates.push("scope_id = ?".to_string());
                match scope.scope_id() {
                    Some(id) => params.push(libsql::Value::Text(id.to_string())),
                    None => params.push(libsql::Value::Null),
                }
            }

            if let Some(event_time) = &input.event_time {
                updates.push("event_time = ?".to_string());
                match event_time {
                    Some(dt) => params.push(libsql::Value::Text(dt.to_rfc3339())),
                    None => params.push(libsql::Value::Null),
                }
            }

            if let Some(trigger_pattern) = &input.trigger_pattern {
                updates.push("trigger_pattern = ?".to_string());
                match trigger_pattern {
                    Some(value) => params.push(libsql::Value::Text(value.clone())),
                    None => params.push(libsql::Value::Null),
                }
            }

            if let Some(ttl) = input.ttl_seconds {
                let mut rows = conn
                    .query(
                        "SELECT tier FROM memories WHERE id = ? AND valid_to IS NULL",
                        libsql::params![id],
                    )
                    .await
                    .map_err(|e| EngramError::Storage(e.to_string()))?;

                let tier_row = rows
                    .next()
                    .await
                    .map_err(|e| EngramError::Storage(e.to_string()))?;

                let tier_str: String = match tier_row {
                    Some(row) => row.get(0).unwrap_or_else(|_| "permanent".to_string()),
                    None => return Err(EngramError::NotFound(id)),
                };

                let tier = tier_str.parse().unwrap_or(MemoryTier::Permanent);

                if ttl <= 0 {
                    if tier == MemoryTier::Daily {
                        return Err(EngramError::InvalidInput(
                            "Cannot remove expiration from a Daily tier memory. Use promote_to_permanent first.".to_string(),
                        ));
                    }
                    updates.push("expires_at = NULL".to_string());
                } else {
                    if tier == MemoryTier::Permanent {
                        return Err(EngramError::InvalidInput(
                            "Cannot set expiration on a Permanent tier memory. Permanent memories cannot expire.".to_string(),
                        ));
                    }
                    let expires_at = (Utc::now() + chrono::Duration::seconds(ttl)).to_rfc3339();
                    updates.push("expires_at = ?".to_string());
                    params.push(libsql::Value::Text(expires_at));
                }
            }

            updates.push("version = version + 1".to_string());
            params.push(libsql::Value::Integer(id));

            let sql = format!(
                "UPDATE memories SET {} WHERE id = ? AND valid_to IS NULL",
                updates.join(", ")
            );

            conn.execute(&sql, params)
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            if let Some(ref tags) = input.tags {
                conn.execute(
                    "DELETE FROM memory_tags WHERE memory_id = ?",
                    libsql::params![id],
                )
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

                for tag in tags {
                    conn.execute(
                        "INSERT OR IGNORE INTO tags (name) VALUES (?)",
                        libsql::params![tag.clone()],
                    )
                    .await
                    .ok();

                    conn.execute(
                        "INSERT OR IGNORE INTO memory_tags (memory_id, tag_id)
                         SELECT ?, id FROM tags WHERE name = ?",
                        libsql::params![id, tag.clone()],
                    )
                    .await
                    .ok();
                }
            }

            drop(conn);

            let sql = format!(
                "SELECT {} FROM memories WHERE id = ? AND valid_to IS NULL",
                MEMORY_COLUMNS
            );
            let mut memories = self
                .query_memories(&sql, vec![libsql::Value::Integer(id)])
                .await?;
            memories.pop().ok_or_else(|| EngramError::NotFound(id))
        })
    }

    fn delete_memory(&self, id: MemoryId) -> Result<()> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            let now = chrono::Utc::now().to_rfc3339();

            // Soft delete by setting valid_to
            let affected = conn
                .execute(
                    "UPDATE memories SET valid_to = ? WHERE id = ? AND valid_to IS NULL",
                    libsql::params![now, id],
                )
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            if affected == 0 {
                return Err(EngramError::NotFound(id));
            }

            Ok(())
        })
    }

    fn list_memories(&self, options: ListOptions) -> Result<Vec<Memory>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let mut sql = format!(
                "SELECT {} FROM memories WHERE valid_to IS NULL",
                MEMORY_COLUMNS
            );
            let mut params: Vec<libsql::Value> = Vec::new();

            if let Some(ref workspace) = options.workspace {
                sql.push_str(" AND workspace = ?");
                params.push(libsql::Value::Text(workspace.clone()));
            } else if let Some(ref workspaces) = options.workspaces {
                if !workspaces.is_empty() {
                    let placeholders = vec!["?"; workspaces.len()].join(", ");
                    sql.push_str(&format!(" AND workspace IN ({})", placeholders));
                    for workspace in workspaces {
                        params.push(libsql::Value::Text(workspace.clone()));
                    }
                }
            }

            if let Some(ref scope) = options.scope {
                sql.push_str(" AND scope_type = ?");
                params.push(libsql::Value::Text(scope.scope_type().to_string()));
                if let Some(scope_id) = scope.scope_id() {
                    sql.push_str(" AND scope_id = ?");
                    params.push(libsql::Value::Text(scope_id.to_string()));
                } else {
                    sql.push_str(" AND scope_id IS NULL");
                }
            }

            if let Some(ref memory_type) = options.memory_type {
                sql.push_str(" AND memory_type = ?");
                params.push(libsql::Value::Text(memory_type.as_str().to_string()));
            }

            if let Some(ref tier) = options.tier {
                sql.push_str(" AND tier = ?");
                params.push(libsql::Value::Text(tier.as_str().to_string()));
            }

            if let Some(ref tags) = options.tags {
                if !tags.is_empty() {
                    let placeholders = vec!["?"; tags.len()].join(", ");
                    sql.push_str(&format!(
                        " AND id IN (
                            SELECT mt.memory_id
                            FROM memory_tags mt
                            JOIN tags t ON t.id = mt.tag_id
                            WHERE t.name IN ({})
                            GROUP BY mt.memory_id
                            HAVING COUNT(DISTINCT t.name) = ?
                        )",
                        placeholders
                    ));
                    for tag in tags {
                        params.push(libsql::Value::Text(tag.clone()));
                    }
                    params.push(libsql::Value::Integer(tags.len() as i64));
                }
            }

            if !options.include_archived {
                sql.push_str(" AND (lifecycle_state IS NULL OR lifecycle_state != 'archived')");
            }

            let sort_field = options.sort_by.unwrap_or(SortField::CreatedAt);
            let sort_order = options.sort_order.unwrap_or(SortOrder::Desc);
            let sort_column = match sort_field {
                SortField::CreatedAt => "created_at",
                SortField::UpdatedAt => "updated_at",
                SortField::LastAccessedAt => "last_accessed_at",
                SortField::Importance => "importance",
                SortField::AccessCount => "access_count",
            };
            let sort_dir = match sort_order {
                SortOrder::Asc => "ASC",
                SortOrder::Desc => "DESC",
            };
            sql.push_str(&format!(" ORDER BY {} {}", sort_column, sort_dir));

            if let Some(limit) = options.limit {
                sql.push_str(&format!(" LIMIT {}", limit));
            }

            if let Some(offset) = options.offset {
                sql.push_str(&format!(" OFFSET {}", offset));
            }

            self.query_memories(&sql, params).await
        })
    }

    fn count_memories(&self, options: ListOptions) -> Result<i64> {
        let mut options = options;
        options.limit = None;
        options.offset = None;
        let memories = self.list_memories(options)?;
        Ok(memories.len() as i64)
    }

    fn search_memories(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            // Simple LIKE-based search (full hybrid search would need vector support)
            let mut sql = format!(
                "SELECT {} FROM memories WHERE valid_to IS NULL AND content LIKE ?",
                MEMORY_COLUMNS
            );
            let mut params = vec![libsql::Value::Text(format!("%{}%", query))];

            if !options.include_archived {
                sql.push_str(" AND (lifecycle_state IS NULL OR lifecycle_state != 'archived')");
            }

            if let Some(ref workspace) = options.workspace {
                sql.push_str(" AND workspace = ?");
                params.push(libsql::Value::Text(workspace.clone()));
            } else if let Some(ref workspaces) = options.workspaces {
                if !workspaces.is_empty() {
                    let placeholders = vec!["?"; workspaces.len()].join(", ");
                    sql.push_str(&format!(" AND workspace IN ({})", placeholders));
                    for workspace in workspaces {
                        params.push(libsql::Value::Text(workspace.clone()));
                    }
                }
            }

            if let Some(ref scope) = options.scope {
                sql.push_str(" AND scope_type = ?");
                params.push(libsql::Value::Text(scope.scope_type().to_string()));
                if let Some(scope_id) = scope.scope_id() {
                    sql.push_str(" AND scope_id = ?");
                    params.push(libsql::Value::Text(scope_id.to_string()));
                } else {
                    sql.push_str(" AND scope_id IS NULL");
                }
            }

            if let Some(ref memory_type) = options.memory_type {
                sql.push_str(" AND memory_type = ?");
                params.push(libsql::Value::Text(memory_type.as_str().to_string()));
            } else if !options.include_transcripts {
                sql.push_str(" AND memory_type != 'transcript_chunk'");
            }

            if let Some(ref tier) = options.tier {
                sql.push_str(" AND tier = ?");
                params.push(libsql::Value::Text(tier.as_str().to_string()));
            }

            sql.push_str(" ORDER BY importance DESC");
            if let Some(limit) = options.limit {
                sql.push_str(&format!(" LIMIT {}", limit));
            } else {
                sql.push_str(" LIMIT 20");
            }

            let memories = self.query_memories(&sql, params).await?;

            Ok(memories
                .into_iter()
                .map(|memory| SearchResult {
                    memory,
                    score: 1.0,
                    match_info: MatchInfo {
                        strategy: SearchStrategy::KeywordOnly,
                        matched_terms: vec![query.to_string()],
                        highlights: Vec::new(),
                        semantic_score: None,
                        keyword_score: Some(1.0),
                    },
                })
                .collect())
        })
    }

    fn create_crossref(
        &self,
        from_id: MemoryId,
        to_id: MemoryId,
        edge_type: EdgeType,
        score: f32,
    ) -> Result<CrossReference> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            let now = Utc::now();
            let now_str = now.to_rfc3339();

            conn.execute(
                "INSERT OR REPLACE INTO crossrefs
                 (from_id, to_id, edge_type, score, confidence, strength, source, source_context, created_at, valid_from, pinned, metadata)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                libsql::params![
                    from_id,
                    to_id,
                    edge_type.as_str(),
                    score as f64,
                    1.0f64,
                    score as f64,
                    "auto",
                    Option::<String>::None,
                    now_str.clone(),
                    now_str,
                    0i64,
                    "{}",
                ],
            )
            .await
            .map_err(|e| EngramError::Storage(e.to_string()))?;

            Ok(CrossReference {
                from_id,
                to_id,
                edge_type,
                score,
                confidence: 1.0,
                strength: score,
                source: RelationSource::Auto,
                source_context: None,
                created_at: now,
                valid_from: now,
                valid_to: None,
                pinned: false,
                metadata: HashMap::new(),
            })
        })
    }

    fn get_crossrefs(&self, memory_id: MemoryId) -> Result<Vec<CrossReference>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.read().await;
            let mut stmt = conn
                .prepare(
                    "SELECT from_id, to_id, edge_type, score, confidence, strength, source,
                        source_context, created_at, valid_from, valid_to, pinned, metadata
                 FROM crossrefs WHERE (from_id = ? OR to_id = ?) AND valid_to IS NULL",
                )
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let rows = stmt
                .query(libsql::params![memory_id, memory_id])
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let mut crossrefs = Vec::new();
            let mut rows = rows;

            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?
            {
                let edge_type_str: String = row.get(2).unwrap_or_else(|_| "related_to".to_string());
                let source_str: String = row.get(6).unwrap_or_else(|_| "auto".to_string());
                let created_at_str: String = row.get(8).unwrap_or_else(|_| Utc::now().to_rfc3339());
                let valid_from_str: String = row.get(9).unwrap_or_else(|_| Utc::now().to_rfc3339());
                let valid_to_str: Option<String> = row.get(10).unwrap_or(None);
                let metadata_str: String = row.get(12).unwrap_or_else(|_| "{}".to_string());
                crossrefs.push(CrossReference {
                    from_id: row.get(0).unwrap_or(0),
                    to_id: row.get(1).unwrap_or(0),
                    edge_type: edge_type_str.parse().unwrap_or(EdgeType::RelatedTo),
                    score: row.get::<f64>(3).unwrap_or(0.0) as f32,
                    confidence: row.get::<f64>(4).unwrap_or(1.0) as f32,
                    strength: row.get::<f64>(5).unwrap_or(1.0) as f32,
                    source: match source_str.as_str() {
                        "manual" => RelationSource::Manual,
                        "llm" => RelationSource::Llm,
                        _ => RelationSource::Auto,
                    },
                    source_context: row.get(7).ok(),
                    created_at: DateTime::parse_from_rfc3339(&created_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    valid_from: DateTime::parse_from_rfc3339(&valid_from_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    valid_to: valid_to_str.and_then(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .map(|dt| dt.with_timezone(&Utc))
                            .ok()
                    }),
                    pinned: row.get::<i64>(11).unwrap_or(0) != 0,
                    metadata: serde_json::from_str(&metadata_str).unwrap_or_default(),
                });
            }

            Ok(crossrefs)
        })
    }

    fn delete_crossref(&self, from_id: MemoryId, to_id: MemoryId) -> Result<()> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            let now = chrono::Utc::now().to_rfc3339();

            conn.execute(
                "UPDATE crossrefs SET valid_to = ? WHERE from_id = ? AND to_id = ? AND valid_to IS NULL",
                libsql::params![now, from_id, to_id],
            ).await.map_err(|e| EngramError::Storage(e.to_string()))?;

            Ok(())
        })
    }

    fn list_tags(&self) -> Result<Vec<(String, i64)>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.read().await;
            let mut stmt = conn
                .prepare(
                    "SELECT t.name, COUNT(mt.memory_id) as count
                 FROM tags t
                 LEFT JOIN memory_tags mt ON t.id = mt.tag_id
                 LEFT JOIN memories m ON mt.memory_id = m.id AND m.valid_to IS NULL
                 GROUP BY t.id, t.name
                 ORDER BY count DESC",
                )
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let rows = stmt
                .query(())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            let mut tags = Vec::new();
            let mut rows = rows;

            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?
            {
                let name: String = row.get(0).unwrap_or_default();
                let count: i64 = row.get(1).unwrap_or(0);
                tags.push((name, count));
            }

            Ok(tags)
        })
    }

    fn get_memories_by_tag(&self, tag: &str, limit: Option<usize>) -> Result<Vec<Memory>> {
        self.list_memories(ListOptions {
            tags: Some(vec![tag.to_string()]),
            limit: limit.map(|l| l as i64),
            ..Default::default()
        })
    }

    fn list_workspaces(&self) -> Result<Vec<(String, i64)>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.read().await;
            let mut stmt = conn.prepare(
                "SELECT workspace, COUNT(*) FROM memories WHERE valid_to IS NULL GROUP BY workspace"
            ).await.map_err(|e| EngramError::Storage(e.to_string()))?;

            let rows = stmt
                .query(())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            let mut workspaces = Vec::new();
            let mut rows = rows;

            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?
            {
                let name: String = row.get(0).unwrap_or_else(|_| "default".to_string());
                let count: i64 = row.get(1).unwrap_or(0);
                workspaces.push((name, count));
            }

            Ok(workspaces)
        })
    }

    fn get_workspace_stats(&self, workspace: &str) -> Result<HashMap<String, i64>> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.read().await;

            let total: i64 = conn.query(
                "SELECT COUNT(*) FROM memories WHERE workspace = ? AND valid_to IS NULL",
                libsql::params![workspace.to_string()],
            ).await.map_err(|e| EngramError::Storage(e.to_string()))?
                .next().await.ok().flatten()
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);

            let permanent: i64 = conn.query(
                "SELECT COUNT(*) FROM memories WHERE workspace = ? AND tier = 'permanent' AND valid_to IS NULL",
                libsql::params![workspace.to_string()],
            ).await.map_err(|e| EngramError::Storage(e.to_string()))?
                .next().await.ok().flatten()
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);

            let daily: i64 = conn.query(
                "SELECT COUNT(*) FROM memories WHERE workspace = ? AND tier = 'daily' AND valid_to IS NULL",
                libsql::params![workspace.to_string()],
            ).await.map_err(|e| EngramError::Storage(e.to_string()))?
                .next().await.ok().flatten()
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);

            let mut stats = HashMap::new();
            stats.insert("memory_count".to_string(), total);
            stats.insert("permanent_count".to_string(), permanent);
            stats.insert("daily_count".to_string(), daily);
            Ok(stats)
        })
    }

    fn move_to_workspace(&self, ids: Vec<MemoryId>, workspace: &str) -> Result<usize> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            let mut moved = 0usize;

            for id in ids {
                let result = conn
                    .execute(
                        "UPDATE memories SET workspace = ? WHERE id = ? AND valid_to IS NULL",
                        libsql::params![workspace.to_string(), id],
                    )
                    .await;

                if result.is_ok() {
                    moved += 1;
                }
            }

            Ok(moved)
        })
    }

    fn get_stats(&self) -> Result<StorageStats> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.read().await;

            let memory_count: i64 = conn
                .query("SELECT COUNT(*) FROM memories WHERE valid_to IS NULL", ())
                .await
                .ok()
                .and_then(|mut r| futures::executor::block_on(r.next()).ok().flatten())
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);

            let crossref_count: i64 = conn
                .query("SELECT COUNT(*) FROM crossrefs WHERE valid_to IS NULL", ())
                .await
                .ok()
                .and_then(|mut r| futures::executor::block_on(r.next()).ok().flatten())
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);

            let tag_count: i64 = conn
                .query("SELECT COUNT(DISTINCT tag_id) FROM memory_tags", ())
                .await
                .ok()
                .and_then(|mut r| futures::executor::block_on(r.next()).ok().flatten())
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);

            let schema_version: i32 = conn
                .query("SELECT COALESCE(MAX(version), 0) FROM schema_version", ())
                .await
                .ok()
                .and_then(|mut r| futures::executor::block_on(r.next()).ok().flatten())
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);

            Ok(StorageStats {
                total_memories: memory_count,
                total_tags: tag_count,
                total_crossrefs: crossref_count,
                total_versions: 0,
                total_identities: 0,
                total_entities: 0,
                db_size_bytes: 0,
                memories_with_embeddings: 0,
                memories_pending_embedding: 0,
                last_sync: None,
                sync_pending: false,
                storage_mode: "turso".to_string(),
                schema_version,
                workspaces: HashMap::new(),
                type_counts: HashMap::new(),
                tier_counts: HashMap::new(),
            })
        })
    }

    fn health_check(&self) -> Result<HealthStatus> {
        let start = Instant::now();

        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        let result = rt.block_on(async {
            let conn = self.conn.read().await;
            conn.query("SELECT 1", ()).await
        });

        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

        match result {
            Ok(_) => Ok(HealthStatus {
                healthy: true,
                latency_ms,
                error: None,
                details: HashMap::from([
                    ("backend".to_string(), "turso".to_string()),
                    ("url".to_string(), self.config.url.clone()),
                ]),
            }),
            Err(e) => Ok(HealthStatus {
                healthy: false,
                latency_ms,
                error: Some(e.to_string()),
                details: HashMap::new(),
            }),
        }
    }

    fn optimize(&self) -> Result<()> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            conn.execute("VACUUM", ())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    fn backend_name(&self) -> &'static str {
        "turso"
    }

    fn schema_version(&self) -> Result<i32> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.read().await;
            let version: i32 = conn
                .query("SELECT COALESCE(MAX(version), 0) FROM schema_version", ())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?
                .next()
                .await
                .ok()
                .flatten()
                .map(|r| r.get(0).unwrap_or(0))
                .unwrap_or(0);
            Ok(version)
        })
    }
}

impl TransactionalBackend for TursoBackend {
    fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&dyn StorageBackend) -> Result<T>,
    {
        // libSQL handles transactions automatically for single operations
        // For explicit transactions, we'd need async transaction support
        f(self)
    }

    fn savepoint(&self, name: &str) -> Result<()> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            conn.execute(&format!("SAVEPOINT {}", name), ())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    fn release_savepoint(&self, name: &str) -> Result<()> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            conn.execute(&format!("RELEASE SAVEPOINT {}", name), ())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    fn rollback_to_savepoint(&self, name: &str) -> Result<()> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;

        rt.block_on(async {
            let conn = self.conn.write().await;
            conn.execute(&format!("ROLLBACK TO SAVEPOINT {}", name), ())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }
}

impl CloudSyncBackend for TursoBackend {
    fn push(&self) -> Result<SyncResult> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;
        rt.block_on(self.sync())
    }

    fn pull(&self) -> Result<SyncResult> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| EngramError::Storage("No tokio runtime available".to_string()))?;
        rt.block_on(self.sync())
    }

    fn sync_delta(&self, _since_version: u64) -> Result<SyncDelta> {
        // Turso handles sync internally via embedded replicas
        Ok(SyncDelta {
            created: Vec::new(),
            updated: Vec::new(),
            deleted: Vec::new(),
            version: 0,
        })
    }

    fn sync_state(&self) -> Result<SyncState> {
        Ok(SyncState {
            local_version: 0,
            remote_version: None,
            last_sync: Some(chrono::Utc::now()),
            has_pending_changes: false,
            pending_count: 0,
        })
    }

    fn force_sync(&self) -> Result<SyncResult> {
        self.push()
    }
}

#[cfg(all(test, feature = "turso"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_turso_in_memory() {
        let backend = TursoBackend::in_memory().await.unwrap();
        assert_eq!(backend.backend_name(), "turso");
    }

    #[tokio::test]
    async fn test_turso_health_check() {
        let backend = TursoBackend::in_memory().await.unwrap();
        let health = backend.health_check().unwrap();
        assert!(health.healthy);
    }

    #[tokio::test]
    async fn test_turso_crud() {
        let backend = TursoBackend::in_memory().await.unwrap();

        // Create
        let input = CreateMemoryInput {
            content: "Test memory for Turso".to_string(),
            memory_type: MemoryType::Note,
            tags: vec!["test".to_string()],
            metadata: HashMap::new(),
            importance: Some(0.7),
            scope: MemoryScope::Global,
            workspace: Some("test".to_string()),
            tier: MemoryTier::Permanent,
            defer_embedding: true,
            ttl_seconds: None,
            dedup_mode: crate::types::DedupMode::Allow,
            dedup_threshold: None,
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            summary_of_id: None,
        };

        let memory = backend.create_memory(input).unwrap();
        assert_eq!(memory.content, "Test memory for Turso");

        // Read
        let retrieved = backend.get_memory(memory.id).unwrap();
        assert!(retrieved.is_some());

        // Update
        let update = UpdateMemoryInput {
            content: Some("Updated Turso memory".to_string()),
            memory_type: None,
            tags: None,
            metadata: None,
            importance: None,
            scope: None,
            ttl_seconds: None,
            event_time: None,
            trigger_pattern: None,
        };
        let updated = backend.update_memory(memory.id, update).unwrap();
        assert_eq!(updated.content, "Updated Turso memory");

        // Delete
        backend.delete_memory(memory.id).unwrap();
        let deleted = backend.get_memory(memory.id).unwrap();
        assert!(deleted.is_none());
    }
}
