//! Engram Snapshots (.egm) — portable knowledge packages
//!
//! Create, load, and inspect .egm snapshot files for distributing
//! curated AI knowledge bases between agents and deployments.
//!
//! # Overview
//!
//! A `.egm` file is a ZIP archive containing:
//! - `manifest.json` — metadata, integrity hash, version info
//! - `memories.json` — all memory records
//! - `entities.json` — extracted named entities
//! - `graph_edges.json` — typed relationships between memories
//! - `README.md` — human-readable description
//!
//! Optionally:
//! - `payload.enc` — AES-256-GCM encrypted payload (instead of plain files)
//! - `manifest.sig` — Ed25519 signature of the manifest
//!
//! # Usage
//!
//! ```rust,ignore
//! use engram::snapshot::{SnapshotBuilder, SnapshotLoader, LoadStrategy};
//! use engram::storage::Storage;
//! use std::path::Path;
//!
//! // Create a snapshot
//! let storage = Storage::open_in_memory().unwrap();
//! let manifest = SnapshotBuilder::new(storage.clone())
//!     .workspace("my-project")
//!     .description("My knowledge base")
//!     .build(Path::new("knowledge.egm"))
//!     .unwrap();
//!
//! // Inspect without loading
//! let info = SnapshotLoader::inspect(Path::new("knowledge.egm")).unwrap();
//! println!("{} memories", info.manifest.memory_count);
//!
//! // Load into another instance
//! let result = SnapshotLoader::load(
//!     &storage,
//!     Path::new("knowledge.egm"),
//!     LoadStrategy::Merge,
//!     Some("imported"),
//!     None,
//! ).unwrap();
//! println!("{} memories loaded", result.memories_loaded);
//! ```

pub mod builder;
pub mod crypto;
pub mod loader;
pub mod types;

pub use builder::SnapshotBuilder;
pub use crypto::{decrypt_aes256, encrypt_aes256, sign_ed25519, verify_ed25519};
pub use loader::SnapshotLoader;
pub use types::{LoadResult, LoadStrategy, SnapshotInfo, SnapshotManifest};
