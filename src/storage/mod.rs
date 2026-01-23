//! Storage engine for Engram
//!
//! Handles SQLite database operations, WAL mode, and schema management.

mod audit;
mod confidence;
mod connection;
mod migrations;
pub mod queries;
pub mod temporal;

pub use audit::*;
pub use confidence::*;
pub use connection::{Storage, StoragePool};
pub use temporal::{
    MemorySnapshot, StateDiff, TemporalMemory, TemporalQueryEngine, TemporalQueryOptions,
};
