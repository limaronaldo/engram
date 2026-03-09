//! Attestation type definitions

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single attestation record proving document ingestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationRecord {
    pub id: Option<i64>,
    pub document_hash: String,
    pub document_name: String,
    pub document_size: usize,
    pub ingested_at: DateTime<Utc>,
    pub agent_id: Option<String>,
    pub memory_ids: Vec<i64>,
    pub previous_hash: String,
    pub record_hash: String,
    /// Hex-encoded Ed25519 signature (optional)
    pub signature: Option<String>,
    pub metadata: serde_json::Value,
    pub created_at: Option<DateTime<Utc>>,
}

/// Result of chain verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChainStatus {
    /// Chain is valid — all hashes check out
    Valid { record_count: usize },
    /// Chain is broken at a specific record
    Broken {
        at_record_id: i64,
        expected_hash: String,
        actual_hash: String,
    },
    /// No records in chain
    Empty,
}

/// A Merkle proof for a single attestation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    pub leaf_hash: String,
    pub leaf_index: usize,
    /// (hash, is_right_sibling) pairs along path from leaf to root
    pub proof_hashes: Vec<(String, bool)>,
    pub root_hash: String,
    pub total_leaves: usize,
}

/// Filter options for listing attestation records
#[derive(Debug, Clone, Default)]
pub struct AttestationFilter {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub agent_id: Option<String>,
    pub document_name: Option<String>,
}
