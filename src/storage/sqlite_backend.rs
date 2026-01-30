//! SQLite implementation of the StorageBackend trait (ENG-15)
//!
//! This module provides a SQLite-based storage backend that implements
//! the `StorageBackend` trait, allowing the existing SQLite storage
//! to be used through the abstracted interface.

use std::collections::HashMap;
use std::time::Instant;

use crate::error::Result;
use crate::types::{
    CreateCrossRefInput, CreateMemoryInput, CrossReference, EdgeType, ListOptions, Memory,
    MemoryId, SearchOptions, SearchResult, StorageConfig, UpdateMemoryInput, WorkspaceStats,
};

use super::backend::{
    BatchCreateResult, BatchDeleteResult, CloudSyncBackend, HealthStatus, StorageBackend,
    StorageStats, SyncDelta, SyncResult, SyncState, TransactionalBackend,
};
use super::connection::Storage;
use super::queries::{
    self, delete_memory_batch, get_related, get_sync_delta, get_sync_version, list_tags,
};
use crate::search::{hybrid_search, SearchConfig};

/// SQLite-based storage backend
///
/// This implements the `StorageBackend` trait using SQLite as the
/// underlying database. It wraps the existing `Storage` struct and
/// delegates to the functions in `queries.rs`.
pub struct SqliteBackend {
    storage: Storage,
}

impl SqliteBackend {
    /// Create a new SQLite backend with the given configuration
    pub fn new(config: StorageConfig) -> Result<Self> {
        let storage = Storage::open(config)?;
        Ok(Self { storage })
    }

    /// Create an in-memory SQLite backend (useful for testing)
    pub fn in_memory() -> Result<Self> {
        let storage = Storage::open_in_memory()?;
        Ok(Self { storage })
    }

    /// Get a reference to the underlying Storage
    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    /// Get a mutable reference to the underlying Storage
    pub fn storage_mut(&mut self) -> &mut Storage {
        &mut self.storage
    }
}

impl StorageBackend for SqliteBackend {
    fn create_memory(&self, input: CreateMemoryInput) -> Result<Memory> {
        self.storage.with_transaction(|conn| {
            queries::create_memory(conn, &input)
        })
    }

    fn get_memory(&self, id: MemoryId) -> Result<Option<Memory>> {
        self.storage
            .with_connection(|conn| match queries::get_memory(conn, id) {
                Ok(memory) => Ok(Some(memory)),
                Err(crate::error::EngramError::NotFound(_)) => Ok(None),
                Err(e) => Err(e),
            })
    }

    fn update_memory(&self, id: MemoryId, input: UpdateMemoryInput) -> Result<Memory> {
        self.storage.with_transaction(|conn| {
            queries::update_memory(conn, id, &input)
        })
    }

    fn delete_memory(&self, id: MemoryId) -> Result<()> {
        self.storage
            .with_transaction(|conn| queries::delete_memory(conn, id))
    }

    fn create_memories_batch(&self, inputs: Vec<CreateMemoryInput>) -> Result<BatchCreateResult> {
        let start = Instant::now();
        let mut created = Vec::new();
        let mut failed = Vec::new();

        self.storage.with_transaction(|conn| {
            for (idx, input) in inputs.into_iter().enumerate() {
                match queries::create_memory(conn, &input) {
                    Ok(memory) => created.push(memory),
                    Err(e) => failed.push((idx, e.to_string())),
                }
            }
            Ok(())
        })?;

        Ok(BatchCreateResult {
            created,
            failed,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn delete_memories_batch(&self, ids: Vec<MemoryId>) -> Result<BatchDeleteResult> {
        self.storage.with_transaction(|conn| {
            let result = delete_memory_batch(conn, &ids)?;
            let mut not_found = Vec::new();
            let mut failed = Vec::new();
            for err in &result.failed {
                if let Some(id) = err.id {
                    let msg = err.error.clone();
                    if msg.to_lowercase().contains("notfound")
                        || msg.to_lowercase().contains("not found")
                    {
                        not_found.push(id);
                    } else {
                        failed.push((id, msg));
                    }
                }
            }
            Ok(BatchDeleteResult {
                deleted_count: result.total_deleted,
                not_found,
                failed,
            })
        })
    }

    fn list_memories(&self, options: ListOptions) -> Result<Vec<Memory>> {
        self.storage
            .with_connection(|conn| queries::list_memories(conn, &options))
    }

    fn search_memories(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>> {
        self.storage.with_connection(|conn| {
            let config = SearchConfig::default();
            hybrid_search(conn, query, None, &options, &config)
        })
    }

    fn count_memories(&self, options: ListOptions) -> Result<i64> {
        self.storage.with_connection(|conn| {
            // Use a count query instead of listing all memories
            let mut sql = String::from("SELECT COUNT(*) FROM memories WHERE 1=1");
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(ref workspace) = options.workspace {
                sql.push_str(" AND workspace = ?");
                params.push(Box::new(workspace.clone()));
            }

            if !options.include_archived {
                sql.push_str(" AND (lifecycle_state IS NULL OR lifecycle_state != 'archived')");
            }

            // Add more filters as needed...

            let count: i64 =
                conn.query_row(&sql, rusqlite::params_from_iter(params.iter()), |row| {
                    row.get(0)
                })?;

            Ok(count)
        })
    }

    fn create_crossref(
        &self,
        from_id: MemoryId,
        to_id: MemoryId,
        edge_type: EdgeType,
        score: f32,
    ) -> Result<CrossReference> {
        self.storage.with_transaction(|conn| {
            let input = CreateCrossRefInput {
                from_id,
                to_id,
                edge_type,
                strength: Some(score),
                source_context: None,
                pinned: false,
            };
            queries::create_crossref(conn, &input)
        })
    }

    fn get_crossrefs(&self, memory_id: MemoryId) -> Result<Vec<CrossReference>> {
        self.storage
            .with_connection(|conn| get_related(conn, memory_id))
    }

    fn delete_crossref(&self, from_id: MemoryId, to_id: MemoryId) -> Result<()> {
        self.storage.with_transaction(|conn| {
            for edge_type in EdgeType::all() {
                let _ = queries::delete_crossref(conn, from_id, to_id, *edge_type);
            }
            Ok(())
        })
    }

    fn list_tags(&self) -> Result<Vec<(String, i64)>> {
        self.storage.with_connection(|conn| {
            let tags = list_tags(conn)?;
            Ok(tags.into_iter().map(|t| (t.name, t.count)).collect())
        })
    }

    fn get_memories_by_tag(&self, tag: &str, limit: Option<usize>) -> Result<Vec<Memory>> {
        self.storage.with_connection(|conn| {
            let options = ListOptions {
                tags: Some(vec![tag.to_string()]),
                limit: limit.map(|v| v as i64),
                ..Default::default()
            };
            queries::list_memories(conn, &options)
        })
    }

    fn list_workspaces(&self) -> Result<Vec<(String, i64)>> {
        self.storage.with_connection(|conn| {
            let workspaces = queries::list_workspaces(conn)?;
            Ok(workspaces
                .into_iter()
                .map(|w| (w.workspace, w.memory_count))
                .collect())
        })
    }

    fn get_workspace_stats(&self, workspace: &str) -> Result<HashMap<String, i64>> {
        self.storage
            .with_connection(|conn| {
                let stats: WorkspaceStats = queries::get_workspace_stats(conn, workspace)?;
                let mut map = HashMap::new();
                map.insert("memory_count".to_string(), stats.memory_count);
                map.insert("permanent_count".to_string(), stats.permanent_count);
                map.insert("daily_count".to_string(), stats.daily_count);
                Ok(map)
            })
    }

    fn move_to_workspace(&self, ids: Vec<MemoryId>, workspace: &str) -> Result<usize> {
        self.storage.with_transaction(|conn| {
            let mut moved = 0usize;
            for id in ids {
                if queries::move_to_workspace(conn, id, workspace).is_ok() {
                    moved += 1;
                }
            }
            Ok(moved)
        })
    }

    fn get_stats(&self) -> Result<StorageStats> {
        self.storage.with_connection(|conn| {
            // Get basic counts
            let memory_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM memories WHERE valid_to IS NULL",
                [],
                |row| row.get(0),
            )?;

            let crossref_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM crossrefs WHERE valid_to IS NULL",
                [],
                |row| row.get(0),
            )?;

            let tag_count: i64 = conn.query_row(
                "SELECT COUNT(DISTINCT tag_id) FROM memory_tags",
                [],
                |row| row.get(0),
            )?;

            let identity_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM identities", [], |row| row.get(0))
                .unwrap_or(0);

            let entity_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))
                .unwrap_or(0);

            // Get schema version
            let schema_version: i32 = conn
                .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);

            // Get database size
            let db_size: i64 = conn.query_row(
                "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
                [],
                |row| row.get(0),
            )?;

            // Get workspace counts
            let mut workspace_stmt = conn.prepare(
                "SELECT workspace, COUNT(*) FROM memories WHERE valid_to IS NULL GROUP BY workspace",
            )?;
            let workspace_counts: HashMap<String, i64> = workspace_stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
                .filter_map(|r| r.ok())
                .collect();

            // Get type counts
            let mut type_stmt = conn.prepare(
                "SELECT memory_type, COUNT(*) FROM memories WHERE valid_to IS NULL GROUP BY memory_type",
            )?;
            let type_counts: HashMap<String, i64> = type_stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
                .filter_map(|r| r.ok())
                .collect();

            // Get tier counts
            let mut tier_stmt =
                conn.prepare("SELECT COALESCE(tier, 'permanent'), COUNT(*) FROM memories GROUP BY COALESCE(tier, 'permanent')")?;
            let tier_counts: HashMap<String, i64> = tier_stmt
                .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
                .filter_map(|r| r.ok())
                .collect();

            Ok(StorageStats {
                memory_count,
                crossref_count,
                tag_count,
                identity_count,
                entity_count,
                db_size_bytes: Some(db_size),
                storage_mode: format!("sqlite-{:?}", self.storage.storage_mode()).to_lowercase(),
                schema_version,
                workspace_counts,
                type_counts,
                tier_counts,
            })
        })
    }

    fn health_check(&self) -> Result<HealthStatus> {
        let start = Instant::now();

        let result = self.storage.with_connection(|conn| {
            // Simple query to check database health
            conn.query_row("SELECT 1", [], |_| Ok(()))?;
            Ok(())
        });

        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;

        match result {
            Ok(()) => Ok(HealthStatus {
                healthy: true,
                latency_ms,
                error: None,
                details: HashMap::from([
                    ("db_path".to_string(), self.storage.db_path().to_string()),
                    (
                        "storage_mode".to_string(),
                        format!("{:?}", self.storage.storage_mode()),
                    ),
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
        self.storage.vacuum()?;
        self.storage.checkpoint()?;
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "sqlite"
    }

    fn schema_version(&self) -> Result<i32> {
        self.storage.with_connection(|conn| {
            let version: i32 = conn
                .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                    row.get(0)
                })
                .unwrap_or(0);
            Ok(version)
        })
    }
}

impl TransactionalBackend for SqliteBackend {
    fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&dyn StorageBackend) -> Result<T>,
    {
        // Note: This is a simplified implementation. For true nested transactions,
        // we would need savepoint support which SQLite provides.
        // For now, we just execute the function with the current backend.
        f(self)
    }

    fn savepoint(&self, name: &str) -> Result<()> {
        self.storage.with_connection(|conn| {
            conn.execute(&format!("SAVEPOINT {}", name), [])?;
            Ok(())
        })
    }

    fn release_savepoint(&self, name: &str) -> Result<()> {
        self.storage.with_connection(|conn| {
            conn.execute(&format!("RELEASE SAVEPOINT {}", name), [])?;
            Ok(())
        })
    }

    fn rollback_to_savepoint(&self, name: &str) -> Result<()> {
        self.storage.with_connection(|conn| {
            conn.execute(&format!("ROLLBACK TO SAVEPOINT {}", name), [])?;
            Ok(())
        })
    }
}

impl CloudSyncBackend for SqliteBackend {
    fn push(&self) -> Result<SyncResult> {
        // Placeholder - actual cloud sync is handled by the sync module
        Ok(SyncResult {
            success: true,
            pushed_count: 0,
            pulled_count: 0,
            conflicts_resolved: 0,
            error: None,
            new_version: 0,
        })
    }

    fn pull(&self) -> Result<SyncResult> {
        // Placeholder - actual cloud sync is handled by the sync module
        Ok(SyncResult {
            success: true,
            pushed_count: 0,
            pulled_count: 0,
            conflicts_resolved: 0,
            error: None,
            new_version: 0,
        })
    }

    fn sync_delta(&self, since_version: u64) -> Result<SyncDelta> {
        self.storage.with_connection(|conn| {
            let delta = get_sync_delta(conn, since_version as i64)?;
            Ok(SyncDelta {
                created: delta.created,
                updated: delta.updated,
                deleted: delta.deleted,
                version: delta.to_version as u64,
            })
        })
    }

    fn sync_state(&self) -> Result<SyncState> {
        self.storage.with_connection(|conn| {
            let version = get_sync_version(conn)?;
            Ok(SyncState {
                local_version: version.version as u64,
                remote_version: None,
                last_sync: Some(version.last_modified),
                has_pending_changes: false,
                pending_count: 0,
            })
        })
    }

    fn force_sync(&self) -> Result<SyncResult> {
        // Push then pull
        self.push()?;
        self.pull()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_in_memory() {
        let backend = SqliteBackend::in_memory().unwrap();
        assert_eq!(backend.backend_name(), "sqlite");
    }

    #[test]
    fn test_health_check() {
        let backend = SqliteBackend::in_memory().unwrap();
        let health = backend.health_check().unwrap();
        assert!(health.healthy);
        assert!(health.latency_ms >= 0.0);
    }

    #[test]
    fn test_get_stats() {
        let backend = SqliteBackend::in_memory().unwrap();
        let stats = backend.get_stats().unwrap();
        assert_eq!(stats.memory_count, 0);
        assert_eq!(stats.backend_name(), "sqlite");
    }

    #[test]
    fn test_crud_operations() {
        let backend = SqliteBackend::in_memory().unwrap();

        // Create
        let input = CreateMemoryInput {
            content: "Test memory".to_string(),
            memory_type: MemoryType::Note,
            tags: vec!["test".to_string()],
            metadata: HashMap::new(),
            importance: Some(0.5),
            scope: MemoryScope::Global,
            workspace: Some("default".to_string()),
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
        assert_eq!(memory.content, "Test memory");
        assert_eq!(memory.memory_type, MemoryType::Note);

        // Read
        let retrieved = backend.get_memory(memory.id).unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, memory.id);

        // Update
        let update_input = UpdateMemoryInput {
            content: Some("Updated memory".to_string()),
            ..Default::default()
        };
        let updated = backend.update_memory(memory.id, update_input).unwrap();
        assert_eq!(updated.content, "Updated memory");

        // Delete
        backend.delete_memory(memory.id).unwrap();
        let deleted = backend.get_memory(memory.id).unwrap();
        assert!(deleted.is_none());
    }
}
