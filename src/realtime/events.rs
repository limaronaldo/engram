//! Real-time event types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::types::MemoryId;

/// Types of real-time events
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    MemoryCreated,
    MemoryUpdated,
    MemoryDeleted,
    CrossrefCreated,
    CrossrefDeleted,
    SyncStarted,
    SyncCompleted,
    SyncFailed,
}

/// A real-time event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealtimeEvent {
    /// Event type
    #[serde(rename = "type")]
    pub event_type: EventType,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Related memory ID (if applicable)
    pub memory_id: Option<MemoryId>,
    /// Preview of content (for created/updated)
    pub preview: Option<String>,
    /// List of changed fields (for updates)
    pub changes: Option<Vec<String>>,
    /// Additional data
    pub data: Option<serde_json::Value>,
}

impl RealtimeEvent {
    /// Create a memory created event
    pub fn memory_created(id: MemoryId, preview: String) -> Self {
        Self {
            event_type: EventType::MemoryCreated,
            timestamp: Utc::now(),
            memory_id: Some(id),
            preview: Some(truncate(&preview, 100)),
            changes: None,
            data: None,
        }
    }

    /// Create a memory updated event
    pub fn memory_updated(id: MemoryId, changes: Vec<String>) -> Self {
        Self {
            event_type: EventType::MemoryUpdated,
            timestamp: Utc::now(),
            memory_id: Some(id),
            preview: None,
            changes: Some(changes),
            data: None,
        }
    }

    /// Create a memory deleted event
    pub fn memory_deleted(id: MemoryId) -> Self {
        Self {
            event_type: EventType::MemoryDeleted,
            timestamp: Utc::now(),
            memory_id: Some(id),
            preview: None,
            changes: None,
            data: None,
        }
    }

    /// Create a sync completed event
    pub fn sync_completed(direction: &str, changes: i64) -> Self {
        Self {
            event_type: EventType::SyncCompleted,
            timestamp: Utc::now(),
            memory_id: None,
            preview: None,
            changes: None,
            data: Some(serde_json::json!({
                "direction": direction,
                "changes": changes,
            })),
        }
    }

    /// Create a sync failed event
    pub fn sync_failed(error: &str) -> Self {
        Self {
            event_type: EventType::SyncFailed,
            timestamp: Utc::now(),
            memory_id: None,
            preview: None,
            changes: None,
            data: Some(serde_json::json!({
                "error": error,
            })),
        }
    }
}

/// Truncate string for preview (UTF-8 safe)
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        // Take max - 3 chars safely, then append "..."
        let truncated: String = s.chars().take(max.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

/// Subscription filter for events
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubscriptionFilter {
    /// Only events for specific memory IDs
    pub memory_ids: Option<Vec<MemoryId>>,
    /// Only events with specific tags
    pub tags: Option<Vec<String>>,
    /// Only specific event types
    pub event_types: Option<Vec<EventType>>,
}

impl SubscriptionFilter {
    /// Check if an event matches this filter
    pub fn matches(&self, event: &RealtimeEvent) -> bool {
        // Check event type filter
        if let Some(ref types) = self.event_types {
            if !types.contains(&event.event_type) {
                return false;
            }
        }

        // Check memory ID filter
        if let Some(ref ids) = self.memory_ids {
            if let Some(event_id) = event.memory_id {
                if !ids.contains(&event_id) {
                    return false;
                }
            }
        }

        // Tags filter would require additional context
        // (memory tags aren't included in events by default)

        true
    }
}
