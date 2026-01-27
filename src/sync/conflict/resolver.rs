//! Conflict resolution strategies and queue

use super::{Conflict, ConflictType, ThreeWayMerge};
use crate::error::Result;
use crate::types::Memory;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Resolution strategy for conflicts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionStrategy {
    /// Keep local version
    KeepLocal,
    /// Keep remote version
    KeepRemote,
    /// Attempt three-way merge
    ThreeWayMerge,
    /// Keep both as separate memories
    KeepBoth,
    /// Take newer version (by timestamp)
    TakeNewer,
    /// Take version with more content
    TakeLonger,
    /// Custom merge provided by user
    CustomMerge,
    /// Union tags, merge metadata, keep local content
    AutoMerge,
}

/// A resolution for a conflict
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resolution {
    /// Strategy used
    pub strategy: ResolutionStrategy,
    /// The resolved memory
    pub resolved_memory: Memory,
    /// When the resolution was made
    pub resolved_at: DateTime<Utc>,
    /// Who/what resolved it
    pub resolved_by: String,
    /// Notes about the resolution
    pub notes: Option<String>,
}

impl Resolution {
    /// Create a new resolution
    pub fn new(
        strategy: ResolutionStrategy,
        memory: Memory,
        resolved_by: impl Into<String>,
    ) -> Self {
        Self {
            strategy,
            resolved_memory: memory,
            resolved_at: Utc::now(),
            resolved_by: resolved_by.into(),
            notes: None,
        }
    }

    /// Add notes to the resolution
    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }
}

/// Conflict resolver with various strategies
pub struct ConflictResolver {
    merger: ThreeWayMerge,
}

impl Default for ConflictResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl ConflictResolver {
    /// Create a new resolver
    pub fn new() -> Self {
        Self {
            merger: ThreeWayMerge::new(),
        }
    }

    /// Resolve a conflict using a strategy
    pub fn resolve(&self, conflict: &Conflict, strategy: ResolutionStrategy) -> Result<Resolution> {
        let resolved_memory = match strategy {
            ResolutionStrategy::KeepLocal => conflict.local.memory.clone(),
            ResolutionStrategy::KeepRemote => conflict.remote.memory.clone(),
            ResolutionStrategy::ThreeWayMerge => self.three_way_merge(conflict)?,
            ResolutionStrategy::KeepBoth => {
                // Return local; caller should also save remote with new ID
                conflict.local.memory.clone()
            }
            ResolutionStrategy::TakeNewer => {
                if conflict.local.created_at > conflict.remote.created_at {
                    conflict.local.memory.clone()
                } else {
                    conflict.remote.memory.clone()
                }
            }
            ResolutionStrategy::TakeLonger => {
                if conflict.local.memory.content.len() >= conflict.remote.memory.content.len() {
                    conflict.local.memory.clone()
                } else {
                    conflict.remote.memory.clone()
                }
            }
            ResolutionStrategy::AutoMerge => self.auto_merge(conflict)?,
            ResolutionStrategy::CustomMerge => {
                // Custom merge should be handled externally
                conflict.local.memory.clone()
            }
        };

        Ok(Resolution::new(strategy, resolved_memory, "system"))
    }

    /// Perform three-way merge
    fn three_way_merge(&self, conflict: &Conflict) -> Result<Memory> {
        let base_content = conflict
            .base
            .as_ref()
            .map(|b| b.memory.content.as_str())
            .unwrap_or("");

        let merge_result = self.merger.merge(
            base_content,
            &conflict.local.memory.content,
            &conflict.remote.memory.content,
        );

        let mut result = conflict.local.memory.clone();
        result.content = merge_result.content;
        result.updated_at = Utc::now();

        // Merge tags
        let base_tags: Vec<String> = conflict
            .base
            .as_ref()
            .map(|b| b.memory.tags.clone())
            .unwrap_or_default();

        result.tags = self.merger.merge_tags(
            &base_tags,
            &conflict.local.memory.tags,
            &conflict.remote.memory.tags,
        );

        // Merge metadata
        let base_meta = conflict.base.as_ref().map(|b| &b.memory.metadata);
        result.metadata = self.merger.merge_metadata_map(
            base_meta,
            &conflict.local.memory.metadata,
            &conflict.remote.memory.metadata,
        );

        Ok(result)
    }

    /// Auto-merge: best for low-severity conflicts
    fn auto_merge(&self, conflict: &Conflict) -> Result<Memory> {
        match conflict.conflict_type {
            ConflictType::MetadataOnly => {
                let mut result = conflict.local.memory.clone();
                let base_meta = conflict.base.as_ref().map(|b| &b.memory.metadata);
                result.metadata = self.merger.merge_metadata_map(
                    base_meta,
                    &conflict.local.memory.metadata,
                    &conflict.remote.memory.metadata,
                );
                result.updated_at = Utc::now();
                Ok(result)
            }
            ConflictType::TagsOnly => {
                let mut result = conflict.local.memory.clone();
                let base_tags: Vec<String> = conflict
                    .base
                    .as_ref()
                    .map(|b| b.memory.tags.clone())
                    .unwrap_or_default();
                result.tags = self.merger.merge_tags(
                    &base_tags,
                    &conflict.local.memory.tags,
                    &conflict.remote.memory.tags,
                );
                result.updated_at = Utc::now();
                Ok(result)
            }
            ConflictType::NonOverlapping => self.three_way_merge(conflict),
            _ => {
                // For content conflicts, try merge but may have markers
                self.three_way_merge(conflict)
            }
        }
    }

    /// Suggest best strategy for a conflict
    pub fn suggest_strategy(&self, conflict: &Conflict) -> ResolutionStrategy {
        match conflict.conflict_type {
            ConflictType::MetadataOnly => ResolutionStrategy::AutoMerge,
            ConflictType::TagsOnly => ResolutionStrategy::AutoMerge,
            ConflictType::NonOverlapping => ResolutionStrategy::ThreeWayMerge,
            ConflictType::ContentConflict => {
                // If one is significantly longer, might prefer that
                let local_len = conflict.local.memory.content.len();
                let remote_len = conflict.remote.memory.content.len();

                if local_len > remote_len * 2 {
                    ResolutionStrategy::KeepLocal
                } else if remote_len > local_len * 2 {
                    ResolutionStrategy::KeepRemote
                } else {
                    ResolutionStrategy::ThreeWayMerge
                }
            }
            ConflictType::DeleteModify => ResolutionStrategy::TakeNewer,
            ConflictType::CreateCreate => ResolutionStrategy::KeepBoth,
        }
    }
}

/// Queue for unresolved conflicts
pub struct ConflictQueue {
    /// Pending conflicts
    conflicts: VecDeque<Conflict>,
    /// Maximum queue size
    max_size: usize,
}

impl Default for ConflictQueue {
    fn default() -> Self {
        Self::new(1000)
    }
}

impl ConflictQueue {
    /// Create a new conflict queue
    pub fn new(max_size: usize) -> Self {
        Self {
            conflicts: VecDeque::new(),
            max_size,
        }
    }

    /// Add a conflict to the queue
    pub fn push(&mut self, conflict: Conflict) -> bool {
        if self.conflicts.len() >= self.max_size {
            return false;
        }
        self.conflicts.push_back(conflict);
        true
    }

    /// Get the next conflict
    pub fn pop(&mut self) -> Option<Conflict> {
        self.conflicts.pop_front()
    }

    /// Peek at the next conflict
    pub fn peek(&self) -> Option<&Conflict> {
        self.conflicts.front()
    }

    /// Get conflict by ID
    pub fn get(&self, id: &str) -> Option<&Conflict> {
        self.conflicts.iter().find(|c| c.id == id)
    }

    /// Remove a conflict by ID
    pub fn remove(&mut self, id: &str) -> Option<Conflict> {
        let pos = self.conflicts.iter().position(|c| c.id == id)?;
        self.conflicts.remove(pos)
    }

    /// Get number of pending conflicts
    pub fn len(&self) -> usize {
        self.conflicts.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.conflicts.is_empty()
    }

    /// Get all conflicts
    pub fn all(&self) -> impl Iterator<Item = &Conflict> {
        self.conflicts.iter()
    }

    /// Get conflicts by memory ID
    pub fn by_memory_id(&self, memory_id: i64) -> Vec<&Conflict> {
        self.conflicts
            .iter()
            .filter(|c| c.memory_id == memory_id)
            .collect()
    }

    /// Get auto-resolvable conflicts
    pub fn auto_resolvable(&self) -> Vec<&Conflict> {
        self.conflicts
            .iter()
            .filter(|c| c.can_auto_resolve())
            .collect()
    }

    /// Clear all conflicts
    pub fn clear(&mut self) {
        self.conflicts.clear();
    }
}

/// Persist conflicts to database
#[allow(dead_code)]
pub fn init_conflict_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS conflicts (
            id TEXT PRIMARY KEY,
            memory_id INTEGER NOT NULL,
            base_version TEXT,
            local_version TEXT NOT NULL,
            remote_version TEXT NOT NULL,
            conflict_type TEXT NOT NULL,
            detected_at TEXT NOT NULL,
            resolved INTEGER NOT NULL DEFAULT 0,
            resolution TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE INDEX IF NOT EXISTS idx_conflicts_memory ON conflicts(memory_id);
        CREATE INDEX IF NOT EXISTS idx_conflicts_resolved ON conflicts(resolved);
        "#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::conflict::SyncMemoryVersion;
    use crate::types::{MemoryType, Visibility};
    use std::collections::HashMap;

    fn create_test_memory(content: &str) -> Memory {
        Memory {
            id: 1,
            content: content.to_string(),
            memory_type: MemoryType::Note,
            tags: vec!["test".to_string()],
            metadata: HashMap::new(),
            importance: 0.5,
            access_count: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: Visibility::Private,
            scope: crate::types::MemoryScope::Global,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None,
        }
    }

    fn create_conflict(local_content: &str, remote_content: &str) -> Conflict {
        let local = SyncMemoryVersion::new(create_test_memory(local_content), "local");
        let remote = SyncMemoryVersion::new(create_test_memory(remote_content), "remote");
        Conflict::new(1, None, local, remote, ConflictType::ContentConflict)
    }

    #[test]
    fn test_resolve_keep_local() {
        let resolver = ConflictResolver::new();
        let conflict = create_conflict("Local content", "Remote content");

        let resolution = resolver
            .resolve(&conflict, ResolutionStrategy::KeepLocal)
            .unwrap();
        assert_eq!(resolution.resolved_memory.content, "Local content");
        assert_eq!(resolution.strategy, ResolutionStrategy::KeepLocal);
    }

    #[test]
    fn test_resolve_keep_remote() {
        let resolver = ConflictResolver::new();
        let conflict = create_conflict("Local content", "Remote content");

        let resolution = resolver
            .resolve(&conflict, ResolutionStrategy::KeepRemote)
            .unwrap();
        assert_eq!(resolution.resolved_memory.content, "Remote content");
    }

    #[test]
    fn test_resolve_take_longer() {
        let resolver = ConflictResolver::new();
        let conflict = create_conflict("Short", "This is much longer content");

        let resolution = resolver
            .resolve(&conflict, ResolutionStrategy::TakeLonger)
            .unwrap();
        assert_eq!(
            resolution.resolved_memory.content,
            "This is much longer content"
        );
    }

    #[test]
    fn test_conflict_queue() {
        let mut queue = ConflictQueue::new(10);

        let c1 = create_conflict("A", "B");
        let c2 = create_conflict("C", "D");
        let id1 = c1.id.clone();

        queue.push(c1);
        queue.push(c2);

        assert_eq!(queue.len(), 2);
        assert!(queue.get(&id1).is_some());

        let popped = queue.pop().unwrap();
        assert_eq!(popped.id, id1);
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_suggest_strategy() {
        let resolver = ConflictResolver::new();

        // Metadata only
        let mut local_mem = create_test_memory("Same");
        local_mem
            .metadata
            .insert("a".to_string(), serde_json::json!(1));

        let mut remote_mem = create_test_memory("Same");
        remote_mem
            .metadata
            .insert("a".to_string(), serde_json::json!(2));

        let local = SyncMemoryVersion::new(local_mem, "local");
        let remote = SyncMemoryVersion::new(remote_mem, "remote");
        let conflict = Conflict::new(1, None, local, remote, ConflictType::MetadataOnly);

        assert_eq!(
            resolver.suggest_strategy(&conflict),
            ResolutionStrategy::AutoMerge
        );
    }
}
