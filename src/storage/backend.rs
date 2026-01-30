use crate::error::EngramError;
pub use crate::types::StorageStats;
use crate::types::{
    CreateMemoryInput, CrossReference, EdgeType, ListOptions, Memory, MemoryId, SearchOptions,
    SearchResult, UpdateMemoryInput,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Result of a batch creation operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCreateResult {
    pub created: Vec<Memory>,
    pub failed: Vec<(usize, String)>,
    pub elapsed_ms: f64,
}

/// Result of a batch deletion operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchDeleteResult {
    pub deleted_count: usize,
    pub not_found: Vec<MemoryId>,
    pub failed: Vec<(MemoryId, String)>,
}

/// Result of a sync operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResult {
    pub success: bool,
    pub pushed_count: usize,
    pub pulled_count: usize,
    pub conflicts_resolved: usize,
    pub error: Option<String>,
    pub new_version: i64,
}

/// Delta of changes for synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncDelta {
    pub created: Vec<Memory>,
    pub updated: Vec<Memory>,
    pub deleted: Vec<MemoryId>,
    pub version: u64,
}

/// Current state of synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncState {
    pub local_version: u64,
    pub remote_version: Option<u64>,
    pub last_sync: Option<chrono::DateTime<chrono::Utc>>,
    pub has_pending_changes: bool,
    pub pending_count: usize,
}

/// Health status of the storage backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub latency_ms: f64,
    pub error: Option<String>,
    pub details: HashMap<String, String>,
}

/// Core storage backend trait for Engram (ENG-14)
///
/// This trait defines the interface for all storage operations, allowing
/// for multiple backend implementations (SQLite, Turso, Meilisearch).
pub trait StorageBackend: Send + Sync {
    // --- Memory CRUD ---

    /// Create a new memory
    fn create_memory(&self, input: CreateMemoryInput) -> Result<Memory, EngramError>;

    /// Get a memory by ID
    fn get_memory(&self, id: MemoryId) -> Result<Option<Memory>, EngramError>;

    /// Update a memory
    fn update_memory(&self, id: MemoryId, input: UpdateMemoryInput) -> Result<Memory, EngramError>;

    /// Delete a memory (soft delete)
    fn delete_memory(&self, id: MemoryId) -> Result<(), EngramError>;

    // --- Batch Operations ---

    /// Create multiple memories in a single transaction
    fn create_memories_batch(
        &self,
        inputs: Vec<CreateMemoryInput>,
    ) -> Result<BatchCreateResult, EngramError>;

    /// Delete multiple memories in a single transaction
    fn delete_memories_batch(&self, ids: Vec<MemoryId>) -> Result<BatchDeleteResult, EngramError>;

    // --- Query Operations ---

    /// List memories with filters
    fn list_memories(&self, options: ListOptions) -> Result<Vec<Memory>, EngramError>;

    /// Count memories matching filters
    fn count_memories(&self, options: ListOptions) -> Result<i64, EngramError>;

    /// Search memories using hybrid search
    fn search_memories(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchResult>, EngramError>;

    // --- Graph Operations ---

    /// Create a cross-reference between memories
    fn create_crossref(
        &self,
        from_id: MemoryId,
        to_id: MemoryId,
        edge_type: EdgeType,
        score: f32,
    ) -> Result<CrossReference, EngramError>;

    /// Get cross-references for a memory
    fn get_crossrefs(&self, memory_id: MemoryId) -> Result<Vec<CrossReference>, EngramError>;

    /// Delete a cross-reference
    fn delete_crossref(&self, from_id: MemoryId, to_id: MemoryId) -> Result<(), EngramError>;

    // --- Tag Operations ---

    /// List all tags with usage counts
    fn list_tags(&self) -> Result<Vec<(String, i64)>, EngramError>;

    /// Get memories with a specific tag
    fn get_memories_by_tag(
        &self,
        tag: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Memory>, EngramError>;

    // --- Workspace Operations ---

    /// List all workspaces with memory counts
    fn list_workspaces(&self) -> Result<Vec<(String, i64)>, EngramError>;

    /// Get detailed statistics for a workspace
    fn get_workspace_stats(&self, workspace: &str) -> Result<HashMap<String, i64>, EngramError>;

    /// Move memories to a different workspace
    fn move_to_workspace(&self, ids: Vec<MemoryId>, workspace: &str) -> Result<usize, EngramError>;

    // --- Maintenance & Metadata ---

    /// Get storage statistics
    fn get_stats(&self) -> Result<StorageStats, EngramError>;

    /// Check storage health
    fn health_check(&self) -> Result<HealthStatus, EngramError>;

    /// Run optimization (e.g., VACUUM)
    fn optimize(&self) -> Result<(), EngramError>;

    /// Get backend name identifier
    fn backend_name(&self) -> &'static str;

    /// Get current schema version
    fn schema_version(&self) -> Result<i32, EngramError>;
}

/// Extension trait for backends that support transactions (ENG-18)
pub trait TransactionalBackend: StorageBackend {
    /// Execute a closure within a transaction
    fn with_transaction<F, T>(&self, f: F) -> Result<T, EngramError>
    where
        F: FnOnce(&dyn StorageBackend) -> Result<T, EngramError>;

    /// create a savepoint
    fn savepoint(&self, name: &str) -> Result<(), EngramError>;

    /// release a savepoint
    fn release_savepoint(&self, name: &str) -> Result<(), EngramError>;

    /// rollback to a savepoint
    fn rollback_to_savepoint(&self, name: &str) -> Result<(), EngramError>;
}

/// Extension trait for backends that support cloud synchronization (ENG-16)
pub trait CloudSyncBackend: StorageBackend {
    /// Pull changes from cloud
    fn pull(&self) -> Result<SyncResult, EngramError>;

    /// Push changes to cloud
    fn push(&self) -> Result<SyncResult, EngramError>;

    /// Get delta changes since version
    fn sync_delta(&self, since_version: u64) -> Result<SyncDelta, EngramError>;

    /// Get current sync state
    fn sync_state(&self) -> Result<SyncState, EngramError>;

    /// Force a full sync (push then pull)
    fn force_sync(&self) -> Result<SyncResult, EngramError>;
}
