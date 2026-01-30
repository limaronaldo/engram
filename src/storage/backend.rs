//! Storage backend trait for abstracting storage implementations (ENG-14)
//!
//! This module defines the `StorageBackend` trait that all storage backends
//! must implement. This allows for swapping out the underlying storage engine
//! (SQLite, Turso, Meilisearch, etc.) without changing the application logic.

use crate::error::Result;
use crate::types::{
    CreateMemoryInput, CrossReference, EdgeType, ListOptions, Memory, MemoryId, MemoryScope,
    SearchOptions, SearchResult, UpdateMemoryInput,
};
use std::collections::HashMap;

/// Statistics about the storage backend
#[derive(Debug, Clone, Default)]
pub struct StorageStats {
    /// Total number of memories
    pub memory_count: i64,
    /// Total number of cross-references
    pub crossref_count: i64,
    /// Total number of unique tags
    pub tag_count: i64,
    /// Total number of identities
    pub identity_count: i64,
    /// Total number of entities
    pub entity_count: i64,
    /// Database size in bytes (if applicable)
    pub db_size_bytes: Option<i64>,
    /// Storage mode (e.g., "sqlite-wal", "sqlite-delete", "turso", "meilisearch")
    pub storage_mode: String,
    /// Schema version
    pub schema_version: i32,
    /// Memories per workspace
    pub workspace_counts: HashMap<String, i64>,
    /// Memories per memory type
    pub type_counts: HashMap<String, i64>,
    /// Memories per tier
    pub tier_counts: HashMap<String, i64>,
}

/// Health status of the storage backend
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Whether the backend is healthy
    pub healthy: bool,
    /// Latency of a simple query in milliseconds
    pub latency_ms: f64,
    /// Optional error message if unhealthy
    pub error: Option<String>,
    /// Additional health details
    pub details: HashMap<String, String>,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            healthy: true,
            latency_ms: 0.0,
            error: None,
            details: HashMap::new(),
        }
    }
}

/// Result of a batch create operation
#[derive(Debug, Clone)]
pub struct BatchCreateResult {
    /// Successfully created memories
    pub created: Vec<Memory>,
    /// Indices of inputs that failed (with error messages)
    pub failed: Vec<(usize, String)>,
    /// Total time taken in milliseconds
    pub elapsed_ms: f64,
}

/// Result of a batch delete operation
#[derive(Debug, Clone)]
pub struct BatchDeleteResult {
    /// Number of memories deleted
    pub deleted_count: usize,
    /// IDs that were not found
    pub not_found: Vec<MemoryId>,
    /// IDs that failed to delete (with error messages)
    pub failed: Vec<(MemoryId, String)>,
}

/// Options for listing memories
#[derive(Debug, Clone, Default)]
pub struct ListMemoriesOptions {
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Offset for pagination
    pub offset: Option<usize>,
    /// Filter by workspace
    pub workspace: Option<String>,
    /// Filter by memory scope
    pub scope: Option<MemoryScope>,
    /// Filter by tags (AND)
    pub tags: Option<Vec<String>>,
    /// Filter by memory type
    pub memory_type: Option<String>,
    /// Include archived memories
    pub include_archived: bool,
    /// Sort by field
    pub sort_by: Option<String>,
    /// Sort descending
    pub sort_desc: bool,
}

/// The core storage backend trait (ENG-14)
///
/// All storage implementations must implement this trait to be usable
/// with the Engram memory system. This includes:
/// - SQLite (current implementation)
/// - Turso (Phase 6)
/// - Meilisearch (Phase 7)
/// - PostgreSQL (future)
///
/// # Design Principles
///
/// 1. **Sync Interface**: All methods are synchronous. Async wrappers can be
///    added at a higher level if needed (e.g., with tokio::spawn_blocking).
///
/// 2. **Error Handling**: All methods return `Result<T>` using the crate's
///    error type for consistent error handling.
///
/// 3. **Immutable Self**: Methods take `&self` to allow for connection pooling
///    and internal mutability patterns.
///
/// 4. **Minimal Dependencies**: The trait uses only types from `crate::types`
///    to avoid coupling to specific implementations.
pub trait StorageBackend: Send + Sync {
    // ========================================================================
    // Core CRUD Operations
    // ========================================================================

    /// Create a new memory
    ///
    /// # Arguments
    /// * `input` - The memory creation input
    ///
    /// # Returns
    /// The created memory with assigned ID and timestamps
    fn create_memory(&self, input: CreateMemoryInput) -> Result<Memory>;

    /// Get a memory by ID
    ///
    /// # Arguments
    /// * `id` - The memory ID
    ///
    /// # Returns
    /// The memory if found, None otherwise
    fn get_memory(&self, id: MemoryId) -> Result<Option<Memory>>;

    /// Update an existing memory
    ///
    /// # Arguments
    /// * `id` - The memory ID to update
    /// * `input` - The update input (only non-None fields are updated)
    ///
    /// # Returns
    /// The updated memory
    fn update_memory(&self, id: MemoryId, input: UpdateMemoryInput) -> Result<Memory>;

    /// Delete a memory by ID
    ///
    /// # Arguments
    /// * `id` - The memory ID to delete
    ///
    /// # Returns
    /// Ok(()) if deleted, error if not found or deletion failed
    fn delete_memory(&self, id: MemoryId) -> Result<()>;

    // ========================================================================
    // Batch Operations (ENG-17)
    // ========================================================================

    /// Create multiple memories in a single operation
    ///
    /// # Arguments
    /// * `inputs` - Vector of memory creation inputs
    ///
    /// # Returns
    /// Result containing created memories and any failures
    fn create_memories_batch(&self, inputs: Vec<CreateMemoryInput>) -> Result<BatchCreateResult> {
        // Default implementation: create one by one
        let start = std::time::Instant::now();
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

    /// Delete multiple memories in a single operation
    ///
    /// # Arguments
    /// * `ids` - Vector of memory IDs to delete
    ///
    /// # Returns
    /// Result containing deletion counts and failures
    fn delete_memories_batch(&self, ids: Vec<MemoryId>) -> Result<BatchDeleteResult> {
        // Default implementation: delete one by one
        let mut deleted_count = 0;
        let mut not_found = Vec::new();
        let mut failed = Vec::new();

        for id in ids {
            match self.delete_memory(id) {
                Ok(()) => deleted_count += 1,
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("not found") {
                        not_found.push(id);
                    } else {
                        failed.push((id, err_str));
                    }
                }
            }
        }

        Ok(BatchDeleteResult {
            deleted_count,
            not_found,
            failed,
        })
    }

    // ========================================================================
    // Query Operations
    // ========================================================================

    /// List memories with filtering and pagination
    ///
    /// # Arguments
    /// * `options` - List options for filtering, sorting, pagination
    ///
    /// # Returns
    /// Vector of memories matching the criteria
    fn list_memories(&self, options: ListOptions) -> Result<Vec<Memory>>;

    /// Search memories using hybrid search (keyword + semantic)
    ///
    /// # Arguments
    /// * `query` - The search query string
    /// * `options` - Search options (limit, filters, etc.)
    ///
    /// # Returns
    /// Vector of search results with scores
    fn search_memories(&self, query: &str, options: SearchOptions) -> Result<Vec<SearchResult>>;

    /// Count memories matching criteria
    ///
    /// # Arguments
    /// * `options` - List options for filtering (limit/offset ignored)
    ///
    /// # Returns
    /// Count of matching memories
    fn count_memories(&self, options: ListOptions) -> Result<i64> {
        // Default: list and count (inefficient, backends should override)
        let memories = self.list_memories(ListOptions {
            limit: None,
            offset: None,
            ..options
        })?;
        Ok(memories.len() as i64)
    }

    // ========================================================================
    // Cross-Reference Operations
    // ========================================================================

    /// Create a cross-reference between two memories
    fn create_crossref(
        &self,
        from_id: MemoryId,
        to_id: MemoryId,
        edge_type: EdgeType,
        score: f32,
    ) -> Result<CrossReference>;

    /// Get cross-references for a memory
    fn get_crossrefs(&self, memory_id: MemoryId) -> Result<Vec<CrossReference>>;

    /// Delete a cross-reference
    fn delete_crossref(&self, from_id: MemoryId, to_id: MemoryId) -> Result<()>;

    // ========================================================================
    // Tag Operations
    // ========================================================================

    /// Get all tags with counts
    fn list_tags(&self) -> Result<Vec<(String, i64)>>;

    /// Get memories by tag
    fn get_memories_by_tag(&self, tag: &str, limit: Option<usize>) -> Result<Vec<Memory>>;

    // ========================================================================
    // Workspace Operations
    // ========================================================================

    /// List all workspaces with memory counts
    fn list_workspaces(&self) -> Result<Vec<(String, i64)>>;

    /// Get statistics for a specific workspace
    fn get_workspace_stats(&self, workspace: &str) -> Result<HashMap<String, i64>>;

    /// Move memories to a different workspace
    fn move_to_workspace(&self, ids: Vec<MemoryId>, workspace: &str) -> Result<usize>;

    // ========================================================================
    // Maintenance Operations
    // ========================================================================

    /// Get storage statistics
    fn get_stats(&self) -> Result<StorageStats>;

    /// Perform health check
    fn health_check(&self) -> Result<HealthStatus>;

    /// Optimize storage (vacuum, reindex, etc.)
    fn optimize(&self) -> Result<()>;

    /// Get the backend name/type
    fn backend_name(&self) -> &'static str;

    /// Get the schema version
    fn schema_version(&self) -> Result<i32>;
}

/// Extension trait for transaction support (ENG-18)
///
/// Not all backends support transactions (e.g., Meilisearch), so this is
/// a separate trait that can be optionally implemented.
pub trait TransactionalBackend: StorageBackend {
    /// Execute a function within a transaction
    ///
    /// The transaction is committed if the function returns Ok,
    /// and rolled back if it returns Err.
    fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&dyn StorageBackend) -> Result<T>;

    /// Begin a savepoint (nested transaction)
    fn savepoint(&self, name: &str) -> Result<()>;

    /// Release a savepoint
    fn release_savepoint(&self, name: &str) -> Result<()>;

    /// Rollback to a savepoint
    fn rollback_to_savepoint(&self, name: &str) -> Result<()>;
}

/// Extension trait for cloud sync operations (ENG-16)
pub trait CloudSyncBackend: StorageBackend {
    /// Push local changes to cloud storage
    fn push(&self) -> Result<SyncResult>;

    /// Pull remote changes from cloud storage
    fn pull(&self) -> Result<SyncResult>;

    /// Get changes since a version number
    fn sync_delta(&self, since_version: u64) -> Result<SyncDelta>;

    /// Get current sync state
    fn sync_state(&self) -> Result<SyncState>;

    /// Force full sync (push then pull)
    fn force_sync(&self) -> Result<SyncResult>;
}

/// Result of a sync operation
#[derive(Debug, Clone, Default)]
pub struct SyncResult {
    /// Whether the sync was successful
    pub success: bool,
    /// Number of items pushed
    pub pushed_count: usize,
    /// Number of items pulled
    pub pulled_count: usize,
    /// Number of conflicts resolved
    pub conflicts_resolved: usize,
    /// Error message if failed
    pub error: Option<String>,
    /// New sync version after operation
    pub new_version: u64,
}

/// Delta changes for incremental sync
#[derive(Debug, Clone, Default)]
pub struct SyncDelta {
    /// Created memories since version
    pub created: Vec<Memory>,
    /// Updated memories since version
    pub updated: Vec<Memory>,
    /// Deleted memory IDs since version
    pub deleted: Vec<MemoryId>,
    /// Version number of this delta
    pub version: u64,
}

/// Current sync state
#[derive(Debug, Clone, Default)]
pub struct SyncState {
    /// Local version number
    pub local_version: u64,
    /// Remote version number (if known)
    pub remote_version: Option<u64>,
    /// Last sync timestamp
    pub last_sync: Option<chrono::DateTime<chrono::Utc>>,
    /// Whether there are pending local changes
    pub has_pending_changes: bool,
    /// Number of pending changes
    pub pending_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_stats_default() {
        let stats = StorageStats::default();
        assert_eq!(stats.memory_count, 0);
        assert_eq!(stats.crossref_count, 0);
        assert_eq!(stats.schema_version, 0);
    }

    #[test]
    fn test_health_status_default() {
        let status = HealthStatus::default();
        assert!(status.healthy);
        assert!(status.error.is_none());
    }

    #[test]
    fn test_sync_result_default() {
        let result = SyncResult::default();
        assert!(!result.success);
        assert_eq!(result.pushed_count, 0);
    }
}
