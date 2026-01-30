//! Storage engine for Engram
//!
//! Handles SQLite database operations, WAL mode, and schema management.
//!
//! # Architecture (ENG-14)
//!
//! The storage layer is built around the `StorageBackend` trait which defines
//! the interface for all storage operations. This allows for multiple backend
//! implementations:
//!
//! - `SqliteBackend` - Current default, uses rusqlite with WAL mode
//! - `TursoBackend` - Planned for Phase 6, distributed SQLite
//! - `MeilisearchBackend` - Planned for Phase 7, full-text search focused
//!
//! ## Extension Traits
//!
//! - `TransactionalBackend` - For backends that support ACID transactions
//! - `CloudSyncBackend` - For backends with cloud synchronization

mod audit;
pub mod backend;
mod confidence;
mod connection;
pub mod entity_queries;
pub mod filter;
pub mod graph_queries;
pub mod identity_links;
pub mod image_storage;
mod migrations;
pub mod queries;
pub mod sqlite_backend;
pub mod temporal;

#[cfg(feature = "turso")]
pub mod turso_backend;

pub use audit::*;
pub use backend::{
    BatchCreateResult as BackendBatchCreateResult, BatchDeleteResult as BackendBatchDeleteResult,
    CloudSyncBackend, HealthStatus, StorageBackend, StorageStats, SyncDelta as BackendSyncDelta,
    SyncResult, SyncState, TransactionalBackend,
};
pub use confidence::*;
pub use connection::{Storage, StoragePool};
pub use entity_queries::{
    delete_entity, find_entity, get_entities_for_memory, get_entity, get_entity_stats,
    get_memories_for_entity, link_entity_to_memory, list_entities, search_entities,
    unlink_entity_from_memory, upsert_entity, EntityStats,
};
pub use graph_queries::{
    find_path, get_neighborhood, get_related_multi_hop, ConnectionType, TraversalDirection,
    TraversalNode, TraversalOptions, TraversalResult, TraversalStats,
};
pub use identity_links::{
    add_alias, create_identity, delete_identity, get_aliases, get_identity, get_identity_memories,
    get_memory_identities, link_identity_to_memory, list_identities, normalize_alias, remove_alias,
    resolve_alias, search_identities_by_alias, unlink_identity_from_memory, update_identity,
    CreateIdentityInput, Identity, IdentityAlias, IdentityType, MemoryIdentityLink,
};
pub use image_storage::{
    migrate_images, parse_data_uri, upload_image, ImageRef, ImageStorageConfig, LocalImageStorage,
    MigrationResult, UploadedImage,
};
pub use queries::{
    acknowledge_share,
    boost_memory,
    cleanup_sync_data,
    clear_events,
    create_checkpoint,
    create_memory,
    // Batch operations
    create_memory_batch,
    // Special types
    create_section_memory,
    delete_memory_batch,
    // Import/export
    export_memories,
    get_agent_sync_state,
    get_sync_delta,
    // Advanced sync
    get_sync_version,
    get_tag_hierarchy,
    import_memories,
    // Existing exports
    list_memories_compact,
    // Tag utilities
    list_tags,
    poll_events,
    poll_shared_memories,
    rebuild_crossrefs,
    // Maintenance
    rebuild_embeddings,
    // Event system
    record_event,
    // Search variants
    search_by_identity,
    search_sessions,
    // Multi-agent sharing
    share_memory,
    update_agent_sync_state,
    validate_tags,
    AgentSyncState,
    BatchCreateResult,
    BatchDeleteResult,
    CompactMemoryRow,
    ExportData,
    ImportResult,
    MemoryEvent,
    MemoryEventType,
    SharedMemory,
    SyncDelta,
    SyncVersion,
    TagHierarchyNode,
    TagInfo,
    TagValidationResult,
};
pub use sqlite_backend::SqliteBackend;
pub use temporal::{
    MemorySnapshot, StateDiff, TemporalMemory, TemporalQueryEngine, TemporalQueryOptions,
};
#[cfg(feature = "turso")]
pub use turso_backend::{TursoBackend, TursoConfig};
