//! Attestation chain — blockchain-style chained records
//!
//! Manages an append-only chain of attestation records. Each record includes a
//! `previous_hash` pointing to the preceding record, forming a tamper-evident
//! chain similar to a blockchain.

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::error::{EngramError, Result};
use crate::storage::Storage;

use super::types::{AttestationFilter, AttestationRecord, ChainStatus};

/// Genesis sentinel — used as previous_hash for the very first record
const GENESIS_HASH: &str = "genesis";

/// Manages the append-only attestation chain
pub struct AttestationChain {
    storage: Storage,
}

impl AttestationChain {
    /// Create a new chain backed by the given storage
    pub fn new(storage: Storage) -> Self {
        Self { storage }
    }

    /// Log a document ingestion, creating a chained attestation record.
    ///
    /// Steps:
    /// 1. Compute SHA-256 hash of `content`
    /// 2. Get the last record's `record_hash` (or `"genesis"` if first)
    /// 3. Build the record with `previous_hash` = last record's hash
    /// 4. Compute `record_hash` = SHA-256 of the canonical representation
    /// 5. Optionally sign with Ed25519
    /// 6. Insert into `attestation_log` and return the created record
    pub fn log_document(
        &self,
        content: &[u8],
        document_name: &str,
        agent_id: Option<&str>,
        memory_ids: &[i64],
        sign_key: Option<&[u8; 32]>,
    ) -> Result<AttestationRecord> {
        if document_name.trim().is_empty() {
            return Err(EngramError::InvalidInput(
                "document_name must not be empty".to_string(),
            ));
        }

        let document_hash = hash_bytes(content);
        let document_size = content.len();
        let ingested_at = Utc::now();

        let previous_hash = match self.get_last_record()? {
            Some(last) => last.record_hash,
            None => GENESIS_HASH.to_string(),
        };

        let memory_ids_vec: Vec<i64> = memory_ids.to_vec();

        // Build a partial record so we can compute the record_hash
        let mut record = AttestationRecord {
            id: None,
            document_hash,
            document_name: document_name.to_string(),
            document_size,
            ingested_at,
            agent_id: agent_id.map(str::to_string),
            memory_ids: memory_ids_vec,
            previous_hash,
            record_hash: String::new(), // filled in below
            signature: None,
            metadata: serde_json::Value::Object(serde_json::Map::new()),
            created_at: None,
        };

        record.record_hash = Self::compute_record_hash(&record);

        // Optional Ed25519 signature over the record_hash bytes
        if let Some(key_bytes) = sign_key {
            record.signature = Some(sign_record_hash(&record.record_hash, key_bytes)?);
        }

        // Persist
        let record = self.insert_record(record)?;
        Ok(record)
    }

    /// Check whether a document (by content) has already been attested.
    ///
    /// Returns the first matching record, or `None`.
    pub fn verify_document(&self, content: &[u8]) -> Result<Option<AttestationRecord>> {
        let hash = hash_bytes(content);
        self.storage.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, document_hash, document_name, document_size, ingested_at,
                        agent_id, memory_ids, previous_hash, record_hash, signature,
                        metadata, created_at
                 FROM attestation_log
                 WHERE document_hash = ?1
                 ORDER BY id ASC
                 LIMIT 1",
            )?;
            let mut rows = stmt.query(rusqlite::params![hash])?;
            match rows.next()? {
                Some(row) => Ok(Some(row_to_record(row)?)),
                None => Ok(None),
            }
        })
    }

    /// Verify the integrity of the entire chain.
    ///
    /// Walks all records in insertion order and checks:
    /// - `previous_hash` of each record matches the `record_hash` of the preceding one
    /// - Each `record_hash` is correctly computed from the record's fields
    pub fn verify_chain(&self) -> Result<ChainStatus> {
        let records = self.storage.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, document_hash, document_name, document_size, ingested_at,
                        agent_id, memory_ids, previous_hash, record_hash, signature,
                        metadata, created_at
                 FROM attestation_log
                 ORDER BY id ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                row_to_record(row).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
                    )
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(EngramError::Database)
        })?;

        if records.is_empty() {
            return Ok(ChainStatus::Empty);
        }

        let mut expected_previous = GENESIS_HASH.to_string();

        for record in &records {
            // 1. Check linkage
            if record.previous_hash != expected_previous {
                return Ok(ChainStatus::Broken {
                    at_record_id: record.id.unwrap_or(-1),
                    expected_hash: expected_previous,
                    actual_hash: record.previous_hash.clone(),
                });
            }

            // 2. Recompute record_hash and compare
            let recomputed = Self::compute_record_hash(record);
            if recomputed != record.record_hash {
                return Ok(ChainStatus::Broken {
                    at_record_id: record.id.unwrap_or(-1),
                    expected_hash: recomputed,
                    actual_hash: record.record_hash.clone(),
                });
            }

            expected_previous = record.record_hash.clone();
        }

        Ok(ChainStatus::Valid {
            record_count: records.len(),
        })
    }

    /// List attestation records with optional filters
    pub fn list(&self, filter: &AttestationFilter) -> Result<Vec<AttestationRecord>> {
        let limit = filter.limit.unwrap_or(100) as i64;
        let offset = filter.offset.unwrap_or(0) as i64;
        let agent_id = filter.agent_id.clone();
        let document_name = filter.document_name.clone();

        self.storage.with_connection(|conn| {
            // Build a flexible query with optional filters
            let mut conditions: Vec<String> = Vec::new();
            let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

            if let Some(ref aid) = agent_id {
                conditions.push(format!("agent_id = ?{}", param_values.len() + 1));
                param_values.push(Box::new(aid.clone()));
            }
            if let Some(ref name) = document_name {
                conditions.push(format!("document_name LIKE ?{}", param_values.len() + 1));
                param_values.push(Box::new(format!("%{}%", name)));
            }

            let where_clause = if conditions.is_empty() {
                String::new()
            } else {
                format!("WHERE {}", conditions.join(" AND "))
            };

            let limit_idx = param_values.len() + 1;
            let offset_idx = param_values.len() + 2;
            param_values.push(Box::new(limit));
            param_values.push(Box::new(offset));

            let sql = format!(
                "SELECT id, document_hash, document_name, document_size, ingested_at,
                        agent_id, memory_ids, previous_hash, record_hash, signature,
                        metadata, created_at
                 FROM attestation_log
                 {}
                 ORDER BY id ASC
                 LIMIT ?{} OFFSET ?{}",
                where_clause, limit_idx, offset_idx
            );

            let mut stmt = conn.prepare(&sql)?;
            let refs: Vec<&dyn rusqlite::ToSql> =
                param_values.iter().map(|b| b.as_ref()).collect();
            let rows = stmt.query_map(refs.as_slice(), |row| {
                row_to_record(row).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
                    )
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()
                .map_err(EngramError::Database)
        })
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    /// Retrieve the most recently inserted record
    fn get_last_record(&self) -> Result<Option<AttestationRecord>> {
        self.storage.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, document_hash, document_name, document_size, ingested_at,
                        agent_id, memory_ids, previous_hash, record_hash, signature,
                        metadata, created_at
                 FROM attestation_log
                 ORDER BY id DESC
                 LIMIT 1",
            )?;
            let mut rows = stmt.query([])?;
            match rows.next()? {
                Some(row) => Ok(Some(row_to_record(row)?)),
                None => Ok(None),
            }
        })
    }

    /// Insert a record into the database and return it with its assigned `id`
    fn insert_record(&self, record: AttestationRecord) -> Result<AttestationRecord> {
        let memory_ids_json =
            serde_json::to_string(&record.memory_ids).map_err(EngramError::Serialization)?;
        let metadata_json =
            serde_json::to_string(&record.metadata).map_err(EngramError::Serialization)?;

        self.storage.with_transaction(|conn| {
            conn.execute(
                "INSERT INTO attestation_log
                    (document_hash, document_name, document_size, ingested_at,
                     agent_id, memory_ids, previous_hash, record_hash, signature, metadata)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    record.document_hash,
                    record.document_name,
                    record.document_size as i64,
                    record.ingested_at.to_rfc3339(),
                    record.agent_id,
                    memory_ids_json,
                    record.previous_hash,
                    record.record_hash,
                    record.signature,
                    metadata_json,
                ],
            )?;

            let id = conn.last_insert_rowid();
            let created_at_str: String = conn.query_row(
                "SELECT created_at FROM attestation_log WHERE id = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            )?;
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .ok();

            Ok(AttestationRecord {
                id: Some(id),
                created_at,
                ..record
            })
        })
    }

    /// Compute the canonical `record_hash` for a record.
    ///
    /// Hash = SHA-256 of:
    /// `document_hash|document_name|document_size|ingested_at|agent_id|memory_ids|previous_hash`
    pub fn compute_record_hash(record: &AttestationRecord) -> String {
        let canonical = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            record.document_hash,
            record.document_name,
            record.document_size,
            record.ingested_at.to_rfc3339(),
            record.agent_id.as_deref().unwrap_or(""),
            serde_json::to_string(&record.memory_ids).unwrap_or_default(),
            record.previous_hash,
        );
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        format!("sha256:{}", hex::encode(hasher.finalize()))
    }
}

// ─── Standalone helpers ──────────────────────────────────────────────────────

/// Compute SHA-256 hash of raw bytes and return as `"sha256:{hex}"` string
fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

/// Sign the `record_hash` string with an Ed25519 secret key.
/// Returns hex-encoded signature.
///
/// This is only compiled when `agent-portability` is active (which is the only
/// context in which this module exists), so no additional cfg gate is needed.
fn sign_record_hash(record_hash: &str, secret_key_bytes: &[u8; 32]) -> Result<String> {
    use ed25519_dalek::{Signature, Signer, SigningKey};

    let signing_key = SigningKey::from_bytes(secret_key_bytes);
    let signature: Signature = signing_key.sign(record_hash.as_bytes());
    Ok(hex::encode(signature.to_bytes()))
}

/// Deserialise a database row into an `AttestationRecord`
fn row_to_record(row: &rusqlite::Row<'_>) -> Result<AttestationRecord> {
    let id: i64 = row.get(0)?;
    let document_hash: String = row.get(1)?;
    let document_name: String = row.get(2)?;
    let document_size: i64 = row.get(3)?;
    let ingested_at_str: String = row.get(4)?;
    let agent_id: Option<String> = row.get(5)?;
    let memory_ids_json: String = row.get(6)?;
    let previous_hash: String = row.get(7)?;
    let record_hash: String = row.get(8)?;
    let signature: Option<String> = row.get(9)?;
    let metadata_json: String = row.get(10)?;
    let created_at_str: Option<String> = row.get(11)?;

    let ingested_at = chrono::DateTime::parse_from_rfc3339(&ingested_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| EngramError::Storage(format!("invalid ingested_at: {e}")))?;

    let memory_ids: Vec<i64> = serde_json::from_str(&memory_ids_json)
        .map_err(|e| EngramError::Storage(format!("invalid memory_ids JSON: {e}")))?;

    let metadata: serde_json::Value = serde_json::from_str(&metadata_json)
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    let created_at = created_at_str.and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    });

    Ok(AttestationRecord {
        id: Some(id),
        document_hash,
        document_name,
        document_size: document_size as usize,
        ingested_at,
        agent_id,
        memory_ids,
        previous_hash,
        record_hash,
        signature,
        metadata,
        created_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    fn test_chain() -> AttestationChain {
        let storage = Storage::open_in_memory().unwrap();
        AttestationChain::new(storage)
    }

    #[test]
    fn test_log_and_verify_document() {
        let chain = test_chain();
        let content = b"hello, world";
        let record = chain
            .log_document(content, "hello.txt", Some("agent-1"), &[1, 2, 3], None)
            .unwrap();

        assert!(record.id.is_some());
        assert_eq!(record.document_name, "hello.txt");
        assert_eq!(record.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(record.memory_ids, vec![1, 2, 3]);
        assert_eq!(record.previous_hash, GENESIS_HASH);
        assert!(!record.record_hash.is_empty());

        // verify_document should find it
        let found = chain.verify_document(content).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().document_name, "hello.txt");
    }

    #[test]
    fn test_chain_linkage() {
        let chain = test_chain();

        let r1 = chain
            .log_document(b"doc1", "doc1.txt", None, &[], None)
            .unwrap();
        let r2 = chain
            .log_document(b"doc2", "doc2.txt", None, &[], None)
            .unwrap();

        assert_eq!(r1.previous_hash, GENESIS_HASH);
        assert_eq!(r2.previous_hash, r1.record_hash);
    }

    #[test]
    fn test_verify_chain_valid() {
        let chain = test_chain();
        chain
            .log_document(b"a", "a.txt", None, &[], None)
            .unwrap();
        chain
            .log_document(b"b", "b.txt", None, &[], None)
            .unwrap();

        match chain.verify_chain().unwrap() {
            ChainStatus::Valid { record_count } => assert_eq!(record_count, 2),
            other => panic!("expected Valid, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_chain_empty() {
        let chain = test_chain();
        assert!(matches!(chain.verify_chain().unwrap(), ChainStatus::Empty));
    }

    #[test]
    fn test_list_with_filter() {
        let chain = test_chain();
        chain
            .log_document(b"x", "x.txt", Some("agent-A"), &[], None)
            .unwrap();
        chain
            .log_document(b"y", "y.txt", Some("agent-B"), &[], None)
            .unwrap();

        let filter = AttestationFilter {
            agent_id: Some("agent-A".to_string()),
            ..Default::default()
        };
        let results = chain.list(&filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].document_name, "x.txt");
    }

    #[test]
    fn test_empty_document_name_rejected() {
        let chain = test_chain();
        let err = chain.log_document(b"data", "", None, &[], None);
        assert!(err.is_err());
    }

    #[test]
    fn test_compute_record_hash_deterministic() {
        let chain = test_chain();
        let r = chain
            .log_document(b"stable", "stable.txt", None, &[], None)
            .unwrap();
        let recomputed = AttestationChain::compute_record_hash(&r);
        assert_eq!(r.record_hash, recomputed);
    }
}
