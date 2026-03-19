//! Integration tests for Phase L — Agent Portability & Knowledge Packaging.
//!
//! Exercises the complete .egm snapshot + attestation chain workflow end-to-end.
//!
//! Run with:
//!   cargo test --test snapshot_attestation --features agent-portability -- --nocapture

#![cfg(feature = "agent-portability")]

use engram::attestation::{AttestationChain, AttestationFilter};
use engram::attestation::types::ChainStatus;
use engram::snapshot::{LoadStrategy, SnapshotBuilder, SnapshotLoader};
use engram::storage::Storage;
use engram::storage::queries::create_memory;
use engram::types::CreateMemoryInput;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn open_storage() -> Storage {
    Storage::open_in_memory().expect("in-memory storage")
}

fn add_memory(storage: &Storage, content: &str, workspace: &str) -> i64 {
    storage
        .with_transaction(|conn| {
            let input = CreateMemoryInput {
                content: content.to_string(),
                workspace: Some(workspace.to_string()),
                ..Default::default()
            };
            create_memory(conn, &input).map(|m| m.id)
        })
        .expect("create memory")
}

fn tmp_egm(suffix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("engram_test_{}.egm", suffix))
}

// ── Scenario 1: Build and inspect a snapshot ─────────────────────────────────

#[test]
fn scenario_1_build_and_inspect() {
    let storage = open_storage();
    add_memory(&storage, "The quick brown fox", "test_ws");
    add_memory(&storage, "Jumps over the lazy dog", "test_ws");

    let path = tmp_egm("s1");
    let manifest = SnapshotBuilder::new(storage)
        .workspace("test_ws")
        .description("scenario 1 snapshot")
        .build(&path)
        .expect("build snapshot");

    assert_eq!(manifest.memory_count, 2);
    assert!(!manifest.encrypted);
    assert!(!manifest.signed);

    // Inspect without loading
    let info = SnapshotLoader::inspect(&path).expect("inspect snapshot");
    assert_eq!(info.manifest.memory_count, 2);
    assert!(info.file_size_bytes > 0);
    assert!(info.files.iter().any(|f| f.contains("manifest")));

    let _ = std::fs::remove_file(&path);
}

// ── Scenario 2: Load with Isolate strategy ───────────────────────────────────

#[test]
fn scenario_2_load_isolate_strategy() {
    let src = open_storage();
    add_memory(&src, "Memory Alpha", "src_ws");
    add_memory(&src, "Memory Beta", "src_ws");
    add_memory(&src, "Memory Gamma", "src_ws");

    let path = tmp_egm("s2");
    SnapshotBuilder::new(src)
        .workspace("src_ws")
        .build(&path)
        .expect("build snapshot");

    let dst = open_storage();
    let result = SnapshotLoader::load(&dst, &path, LoadStrategy::Isolate, None, None)
        .expect("load snapshot");

    // Isolate strategy creates a new workspace
    assert_eq!(result.memories_loaded, 3);
    assert_eq!(result.memories_skipped, 0);
    assert!(!result.target_workspace.is_empty());
    // The target workspace should be different from the original
    assert_ne!(result.target_workspace, "src_ws");

    let _ = std::fs::remove_file(&path);
}

// ── Scenario 3: Verify snapshot_origin and snapshot_loaded_at are set ────────

#[test]
fn scenario_3_provenance_columns_set() {
    let src = open_storage();
    add_memory(&src, "Provenance test content", "prov_ws");

    let path = tmp_egm("s3");
    SnapshotBuilder::new(src)
        .workspace("prov_ws")
        .build(&path)
        .expect("build snapshot");

    let dst = open_storage();
    let result =
        SnapshotLoader::load(&dst, &path, LoadStrategy::Merge, Some("loaded_ws"), None)
            .expect("load snapshot");

    assert_eq!(result.memories_loaded, 1);

    // Verify snapshot_origin and snapshot_loaded_at are stored
    dst.with_connection(|conn| {
        let mut stmt = conn
            .prepare(
                "SELECT snapshot_origin, snapshot_loaded_at FROM memories
                 WHERE workspace = 'loaded_ws' LIMIT 1",
            )
            .expect("prepare stmt");
        let row = stmt
            .query_row([], |r| {
                Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?))
            })
            .expect("query row");

        let origin = row.0.expect("snapshot_origin should be set");
        let loaded_at = row.1.expect("snapshot_loaded_at should be set");
        assert!(
            origin.ends_with(".egm"),
            "snapshot_origin should be an .egm filename, got: {}",
            origin
        );
        assert!(!loaded_at.is_empty(), "snapshot_loaded_at should be non-empty");
        Ok(())
    })
    .expect("provenance check");

    let _ = std::fs::remove_file(&path);
}

// ── Scenario 4: Verify attestation exists for a loaded snapshot ───────────────

#[test]
fn scenario_4_attestation_after_load() {
    let src = open_storage();
    add_memory(&src, "Attestation test A", "attest_ws");
    add_memory(&src, "Attestation test B", "attest_ws");

    let path = tmp_egm("s4");
    SnapshotBuilder::new(src)
        .workspace("attest_ws")
        .build(&path)
        .expect("build snapshot");

    let dst = open_storage();
    let chain = AttestationChain::new(dst.clone());

    // Log the snapshot file attestation manually (as snapshot_load handler does)
    let archive_bytes = std::fs::read(&path).expect("read archive bytes");
    let record = chain
        .log_document(&archive_bytes, "test_snapshot.egm", Some("test-agent"), &[], None)
        .expect("log_document");

    assert!(!record.document_hash.is_empty());
    assert_eq!(record.document_name, "test_snapshot.egm");
    assert_eq!(record.agent_id.as_deref(), Some("test-agent"));

    // Verify document can be found by content
    let found = chain
        .verify_document(&archive_bytes)
        .expect("verify_document");
    assert!(found.is_some(), "attestation record should exist for snapshot");
    let found_record = found.unwrap();
    assert_eq!(found_record.document_name, "test_snapshot.egm");

    let _ = std::fs::remove_file(&path);
}

// ── Scenario 5: verify_chain returns Valid ────────────────────────────────────

#[test]
fn scenario_5_chain_verify_valid() {
    let storage = open_storage();
    let chain = AttestationChain::new(storage);

    // Empty chain
    let status = chain.verify_chain().expect("verify_chain");
    assert!(
        matches!(status, ChainStatus::Empty),
        "empty chain should be Empty, got {:?}",
        status
    );

    // Add three records
    chain
        .log_document(b"document one content", "doc1.txt", None, &[1], None)
        .expect("log doc1");
    chain
        .log_document(b"document two content", "doc2.txt", None, &[2], None)
        .expect("log doc2");
    chain
        .log_document(b"document three content", "doc3.txt", None, &[3], None)
        .expect("log doc3");

    let status = chain.verify_chain().expect("verify_chain after 3 records");
    assert!(
        matches!(status, ChainStatus::Valid { record_count: 3 }),
        "chain should be Valid with 3 records, got {:?}",
        status
    );
}

// ── Scenario 6: Encrypted snapshot with wrong key returns error ───────────────

#[test]
fn scenario_6_encrypted_wrong_key_fails() {
    let storage = open_storage();
    add_memory(&storage, "Secret memory content", "secret_ws");

    let path = tmp_egm("s6");
    let correct_key = [0x42u8; 32];
    SnapshotBuilder::new(storage)
        .workspace("secret_ws")
        .build_encrypted(&path, &correct_key)
        .expect("build encrypted snapshot");

    let dst = open_storage();
    let wrong_key = [0xFFu8; 32];
    let result =
        SnapshotLoader::load(&dst, &path, LoadStrategy::Merge, None, Some(&wrong_key));

    assert!(
        result.is_err(),
        "loading with wrong key should return an error"
    );

    let _ = std::fs::remove_file(&path);
}

// ── Scenario 7: Signed snapshot — verify the signed flag and build_signed API ─

#[test]
fn scenario_7_signed_snapshot_flag() {
    let storage = open_storage();
    add_memory(&storage, "Signed content", "signed_ws");

    let path = tmp_egm("s7");
    let sign_key = [0x11u8; 32];
    let manifest = SnapshotBuilder::new(storage)
        .workspace("signed_ws")
        .build_signed(&path, &sign_key)
        .expect("build signed snapshot");

    // Manifest should be marked as signed
    assert!(manifest.signed, "manifest.signed should be true");
    assert!(!manifest.encrypted, "signed-only snapshot should not be encrypted");

    // Inspect: signed flag persists in stored manifest
    let info = SnapshotLoader::inspect(&path).expect("inspect signed snapshot");
    assert!(info.manifest.signed, "inspect should report signed=true");

    // The archive should contain a manifest.sig entry
    assert!(
        info.files.iter().any(|f| f.contains("sig")),
        "archive should contain a signature file; files = {:?}",
        info.files
    );

    let _ = std::fs::remove_file(&path);
}

// ── Scenario 8: List attestation records with filter ─────────────────────────

#[test]
fn scenario_8_list_attestation_records() {
    let storage = open_storage();
    let chain = AttestationChain::new(storage);

    chain
        .log_document(b"alpha content", "alpha.txt", Some("agent-1"), &[], None)
        .expect("log alpha");
    chain
        .log_document(b"beta content", "beta.txt", Some("agent-2"), &[], None)
        .expect("log beta");
    chain
        .log_document(b"gamma content", "gamma.txt", Some("agent-1"), &[], None)
        .expect("log gamma");

    // List all
    let all = chain
        .list(&AttestationFilter::default())
        .expect("list all");
    assert_eq!(all.len(), 3);

    // Filter by agent_id
    let agent1_records = chain
        .list(&AttestationFilter {
            agent_id: Some("agent-1".to_string()),
            ..Default::default()
        })
        .expect("list agent-1");
    assert_eq!(agent1_records.len(), 2);
    for r in &agent1_records {
        assert_eq!(r.agent_id.as_deref(), Some("agent-1"));
    }
}

// ── Scenario 9: DryRun does not insert memories ───────────────────────────────

#[test]
fn scenario_9_dry_run_no_insert() {
    let src = open_storage();
    add_memory(&src, "DryRun content A", "dr_ws");
    add_memory(&src, "DryRun content B", "dr_ws");

    let path = tmp_egm("s9");
    SnapshotBuilder::new(src)
        .workspace("dr_ws")
        .build(&path)
        .expect("build snapshot");

    let dst = open_storage();
    let result =
        SnapshotLoader::load(&dst, &path, LoadStrategy::DryRun, Some("dr_target"), None)
            .expect("dry run load");

    // DryRun reports what WOULD happen — memories_loaded is a preview count.
    // It does not actually insert anything; the database must remain empty.
    assert_eq!(result.memories_loaded, 2, "DryRun should report 2 would-be-loaded memories");

    // Confirm the destination is truly empty (no rows inserted)
    let count: i64 = dst
        .with_connection(|conn| {
            conn.query_row("SELECT COUNT(*) FROM memories", [], |r| r.get(0))
                .map_err(engram::error::EngramError::Database)
        })
        .expect("count query");
    assert_eq!(count, 0, "DryRun must leave destination empty");

    let _ = std::fs::remove_file(&path);
}

// ── Scenario 10: Merge strategy skips duplicates ─────────────────────────────

#[test]
fn scenario_10_merge_skips_duplicates() {
    let src = open_storage();
    add_memory(&src, "Unique content for merge test", "merge_ws");

    let path = tmp_egm("s10");
    SnapshotBuilder::new(src)
        .workspace("merge_ws")
        .build(&path)
        .expect("build snapshot");

    // Load once
    let dst = open_storage();
    let first = SnapshotLoader::load(&dst, &path, LoadStrategy::Merge, Some("merged"), None)
        .expect("first load");
    assert_eq!(first.memories_loaded, 1);
    assert_eq!(first.memories_skipped, 0);

    // Load again — same content, should be skipped
    let second = SnapshotLoader::load(&dst, &path, LoadStrategy::Merge, Some("merged"), None)
        .expect("second load");
    assert_eq!(second.memories_loaded, 0);
    assert_eq!(second.memories_skipped, 1);

    let _ = std::fs::remove_file(&path);
}
