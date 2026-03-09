//! Attestation tool handlers (Phase L — agent-portability).
//!
//! Provides 4 MCP tools for cryptographic document attestation:
//! - `attestation_log` — record a document ingestion with a chained proof
//! - `attestation_verify` — check whether a document has been attested
//! - `attestation_chain_verify` — verify integrity of the full attestation chain
//! - `attestation_list` — list attestation records with optional filters / export

use serde_json::{json, Value};

use super::HandlerContext;

use crate::attestation::{export_csv, export_json, export_merkle_proof, AttestationChain,
    AttestationFilter, MerkleTree};

// ── attestation_log ───────────────────────────────────────────────────────────

/// Log a document ingestion into the attestation chain.
///
/// Params:
/// - `content` (string, required) — raw document content to attest
/// - `document_name` (string, required) — human-readable document name
/// - `agent_id` (string, optional) — identifier of the ingesting agent
/// - `memory_ids` (array of i64, optional) — memory IDs created from this document
/// - `sign_key` (string, optional) — hex-encoded 32-byte Ed25519 secret key
pub fn attestation_log(ctx: &HandlerContext, params: Value) -> Value {
    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content is required"}),
    };
    let document_name = match params.get("document_name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => return json!({"error": "document_name is required"}),
    };
    let agent_id: Option<String> = params
        .get("agent_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let memory_ids: Vec<i64> = params
        .get("memory_ids")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();
    let sign_key_hex: Option<String> = params
        .get("sign_key")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    // Parse optional signing key
    let sign_key_bytes: Option<[u8; 32]> = match sign_key_hex {
        Some(ref hex) => match parse_hex_key(hex) {
            Ok(k) => Some(k),
            Err(e) => return json!({"error": e}),
        },
        None => None,
    };

    let chain = AttestationChain::new(ctx.storage.clone());

    match chain.log_document(
        content.as_bytes(),
        &document_name,
        agent_id.as_deref(),
        &memory_ids,
        sign_key_bytes.as_ref(),
    ) {
        Ok(record) => json!({
            "status": "ok",
            "record": record,
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── attestation_verify ────────────────────────────────────────────────────────

/// Verify whether a document has previously been attested.
///
/// Params:
/// - `content` (string, required) — document content to verify
pub fn attestation_verify(ctx: &HandlerContext, params: Value) -> Value {
    let content = match params.get("content").and_then(|v| v.as_str()) {
        Some(c) => c.to_string(),
        None => return json!({"error": "content is required"}),
    };

    let chain = AttestationChain::new(ctx.storage.clone());

    match chain.verify_document(content.as_bytes()) {
        Ok(Some(record)) => json!({
            "attested": true,
            "record": record,
        }),
        Ok(None) => json!({
            "attested": false,
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── attestation_chain_verify ──────────────────────────────────────────────────

/// Verify the integrity of the full attestation chain.
///
/// No parameters required.
pub fn attestation_chain_verify(ctx: &HandlerContext, _params: Value) -> Value {
    let chain = AttestationChain::new(ctx.storage.clone());

    match chain.verify_chain() {
        Ok(status) => json!({
            "status": "ok",
            "chain_status": status,
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── attestation_list ──────────────────────────────────────────────────────────

/// List attestation records with optional filters and export formats.
///
/// Params:
/// - `limit` (i64, optional, default 50)
/// - `offset` (i64, optional, default 0)
/// - `agent_id` (string, optional) — filter by agent
/// - `document_name` (string, optional) — filter by name substring
/// - `export_format` (string, optional) — "json", "csv", or "merkle_proof"
pub fn attestation_list(ctx: &HandlerContext, params: Value) -> Value {
    let limit = params
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(50) as usize;
    let offset = params
        .get("offset")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as usize;
    let agent_id = params
        .get("agent_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let document_name = params
        .get("document_name")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let export_format = params
        .get("export_format")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let filter = AttestationFilter {
        limit: Some(limit),
        offset: Some(offset),
        agent_id,
        document_name,
    };

    let chain = AttestationChain::new(ctx.storage.clone());

    let records = match chain.list(&filter) {
        Ok(r) => r,
        Err(e) => return json!({"error": e.to_string()}),
    };

    match export_format.as_deref() {
        Some("json") => match export_json(&records) {
            Ok(exported) => json!({
                "format": "json",
                "count": records.len(),
                "data": exported,
            }),
            Err(e) => json!({"error": e.to_string()}),
        },
        Some("csv") => match export_csv(&records) {
            Ok(exported) => json!({
                "format": "csv",
                "count": records.len(),
                "data": exported,
            }),
            Err(e) => json!({"error": e.to_string()}),
        },
        Some("merkle_proof") => {
            if records.is_empty() {
                return json!({
                    "format": "merkle_proof",
                    "count": 0,
                    "data": null,
                    "message": "No records to build Merkle tree",
                });
            }
            let tree = MerkleTree::build(&records);
            // Generate proof for the first (index 0) record as a representative
            match tree.generate_proof(0) {
                Some(proof) => match export_merkle_proof(&proof) {
                    Ok(exported) => json!({
                        "format": "merkle_proof",
                        "count": records.len(),
                        "root_hash": tree.root(),
                        "data": exported,
                    }),
                    Err(e) => json!({"error": e.to_string()}),
                },
                None => json!({"error": "Failed to generate Merkle proof"}),
            }
        }
        _ => {
            // Default: return records as structured JSON
            json!({
                "count": records.len(),
                "records": records,
            })
        }
    }
}

// ─── Private helpers ──────────────────────────────────────────────────────────

/// Parse a hex-encoded string into a 32-byte array.
fn parse_hex_key(hex_str: &str) -> std::result::Result<[u8; 32], String> {
    let bytes: Vec<u8> = (0..hex_str.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16))
        .collect::<std::result::Result<Vec<u8>, _>>()
        .map_err(|e| format!("Invalid hex: {}", e))?;
    if bytes.len() != 32 {
        return Err(format!("Key must be 32 bytes, got {}", bytes.len()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}
