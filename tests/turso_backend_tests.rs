//! Turso backend integration tests.
//!
//! These tests run in a separate process to avoid the libsql/rusqlite
//! SQLite initialization conflict that occurs under --all-features.
//!
//! Run with: cargo test --test turso_backend_tests --features turso

#![cfg(feature = "turso")]

use std::collections::HashMap;

use engram::storage::{StorageBackend, TursoBackend};
use engram::types::*;

#[tokio::test]
async fn test_turso_in_memory() {
    let backend = TursoBackend::in_memory().await.unwrap();
    assert_eq!(backend.backend_name(), "turso");
}

#[tokio::test]
async fn test_turso_health_check() {
    let backend = TursoBackend::in_memory().await.unwrap();
    let health = backend.health_check().unwrap();
    assert!(health.healthy);
}

#[tokio::test]
async fn test_turso_crud() {
    let backend = TursoBackend::in_memory().await.unwrap();

    // Create
    let input = CreateMemoryInput {
        content: "Test memory for Turso".to_string(),
        memory_type: MemoryType::Note,
        tags: vec!["test".to_string()],
        metadata: HashMap::new(),
        importance: Some(0.7),
        scope: MemoryScope::Global,
        workspace: Some("test".to_string()),
        tier: MemoryTier::Permanent,
        defer_embedding: true,
        ttl_seconds: None,
        dedup_mode: engram::types::DedupMode::Allow,
        dedup_threshold: None,
        event_time: None,
        event_duration_seconds: None,
        trigger_pattern: None,
        summary_of_id: None,
    };

    let memory = backend.create_memory(input).unwrap();
    assert_eq!(memory.content, "Test memory for Turso");

    // Read
    let retrieved = backend.get_memory(memory.id).unwrap();
    assert!(retrieved.is_some());

    // Update
    let update = UpdateMemoryInput {
        content: Some("Updated Turso memory".to_string()),
        memory_type: None,
        tags: None,
        metadata: None,
        importance: None,
        scope: None,
        ttl_seconds: None,
        event_time: None,
        trigger_pattern: None,
    };
    let updated = backend.update_memory(memory.id, update).unwrap();
    assert_eq!(updated.content, "Updated Turso memory");

    // Delete
    backend.delete_memory(memory.id).unwrap();
    let deleted = backend.get_memory(memory.id).unwrap();
    assert!(deleted.is_none());
}
