//! Storage engine for Engram
//!
//! Handles SQLite database operations, WAL mode, and schema management.

mod audit;
mod confidence;
mod connection;
pub mod entity_queries;
pub mod filter;
pub mod graph_queries;
mod migrations;
pub mod queries;
pub mod temporal;

pub use audit::*;
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
pub use temporal::{
    MemorySnapshot, StateDiff, TemporalMemory, TemporalQueryEngine, TemporalQueryOptions,
};
