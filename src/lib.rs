//! Engram - AI Memory Infrastructure
//!
//! Persistent memory for AI agents with semantic search, cloud sync,
//! and knowledge graph visualization.

pub mod auth;
pub mod embedding;
pub mod error;
pub mod graph;
pub mod intelligence;
pub mod mcp;
pub mod realtime;
pub mod search;
pub mod storage;
pub mod sync;
pub mod types;

pub use error::{EngramError, Result};
pub use storage::Storage;
pub use types::*;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
