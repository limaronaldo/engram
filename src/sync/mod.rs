//! Cloud sync functionality (RML-875)
//!
//! Non-blocking S3/R2/GCS sync with debouncing.
//!
//! # Feature Flags
//!
//! This module requires the `cloud` feature to be enabled for cloud storage backends.
//! The conflict resolution logic is always available.

#[cfg(feature = "cloud")]
mod cloud;
pub mod conflict;
#[cfg(feature = "cloud")]
mod worker;

#[cfg(feature = "cloud")]
pub use cloud::CloudStorage;
pub use conflict::{
    Conflict, ConflictDetector, ConflictInfo, ConflictQueue, ConflictResolver, ConflictType,
    MergeResult, Resolution, ResolutionStrategy, SyncMemoryVersion, ThreeWayMerge,
};
#[cfg(feature = "cloud")]
pub use worker::{get_sync_status, SyncWorker};

use chrono::{DateTime, Utc};

/// Sync direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    Push,
    Pull,
    Bidirectional,
}

/// Sync event for logging/notifications
#[derive(Debug, Clone)]
pub struct SyncEvent {
    pub direction: SyncDirection,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub bytes_transferred: u64,
    pub success: bool,
    pub error: Option<String>,
}
