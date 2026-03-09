//! Export attestation records in JSON, CSV, and Merkle proof formats

use crate::error::{EngramError, Result};

use super::types::{AttestationRecord, MerkleProof};

/// Export records as a pretty-printed JSON array
pub fn export_json(records: &[AttestationRecord]) -> Result<String> {
    serde_json::to_string_pretty(records).map_err(EngramError::Serialization)
}

/// Export records as CSV
///
/// Columns: id, document_hash, document_name, document_size, ingested_at,
///          agent_id, memory_ids, previous_hash, record_hash, signature
pub fn export_csv(records: &[AttestationRecord]) -> Result<String> {
    let mut csv = String::from(
        "id,document_hash,document_name,document_size,ingested_at,\
         agent_id,memory_ids,previous_hash,record_hash,signature\n",
    );

    for r in records {
        let memory_ids_str =
            serde_json::to_string(&r.memory_ids).unwrap_or_else(|_| "[]".to_string());

        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            r.id.map(|id| id.to_string()).unwrap_or_default(),
            r.document_hash,
            escape_csv(&r.document_name),
            r.document_size,
            r.ingested_at.to_rfc3339(),
            r.agent_id.as_deref().unwrap_or(""),
            escape_csv(&memory_ids_str),
            r.previous_hash,
            r.record_hash,
            r.signature.as_deref().unwrap_or(""),
        ));
    }

    Ok(csv)
}

/// Export a Merkle proof as pretty-printed JSON
pub fn export_merkle_proof(proof: &MerkleProof) -> Result<String> {
    serde_json::to_string_pretty(proof).map_err(EngramError::Serialization)
}

/// Escape a string value for inclusion in a CSV field.
///
/// Wraps the value in double-quotes and escapes interior double-quotes if the
/// value contains commas, double-quotes, or newlines.
fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attestation::types::MerkleProof;
    use chrono::Utc;

    fn sample_record() -> AttestationRecord {
        AttestationRecord {
            id: Some(1),
            document_hash: "sha256:abc".to_string(),
            document_name: "report.txt".to_string(),
            document_size: 42,
            ingested_at: Utc::now(),
            agent_id: Some("agent-1".to_string()),
            memory_ids: vec![10, 20],
            previous_hash: "genesis".to_string(),
            record_hash: "sha256:def".to_string(),
            signature: None,
            metadata: serde_json::json!({}),
            created_at: None,
        }
    }

    #[test]
    fn test_export_json_roundtrip() {
        let record = sample_record();
        let json = export_json(&[record.clone()]).unwrap();
        let parsed: Vec<AttestationRecord> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].document_name, "report.txt");
    }

    #[test]
    fn test_export_csv_has_header() {
        let csv = export_csv(&[]).unwrap();
        assert!(csv.starts_with("id,document_hash,document_name"));
    }

    #[test]
    fn test_export_csv_record() {
        let record = sample_record();
        let csv = export_csv(&[record]).unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        // header + 1 data row
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("report.txt"));
        assert!(lines[1].contains("sha256:abc"));
    }

    #[test]
    fn test_export_csv_escapes_commas() {
        let mut record = sample_record();
        record.document_name = "report, final.txt".to_string();
        let csv = export_csv(&[record]).unwrap();
        assert!(csv.contains("\"report, final.txt\""));
    }

    #[test]
    fn test_export_merkle_proof() {
        let proof = MerkleProof {
            leaf_hash: "sha256:aaa".to_string(),
            leaf_index: 0,
            proof_hashes: vec![("sha256:bbb".to_string(), true)],
            root_hash: "sha256:ccc".to_string(),
            total_leaves: 2,
        };
        let json = export_merkle_proof(&proof).unwrap();
        assert!(json.contains("leaf_hash"));
        assert!(json.contains("sha256:aaa"));
    }

    #[test]
    fn test_escape_csv_no_special_chars() {
        assert_eq!(escape_csv("plain"), "plain");
    }

    #[test]
    fn test_escape_csv_with_comma() {
        assert_eq!(escape_csv("a,b"), "\"a,b\"");
    }

    #[test]
    fn test_escape_csv_with_quote() {
        assert_eq!(escape_csv("say \"hi\""), "\"say \"\"hi\"\"\"");
    }
}
