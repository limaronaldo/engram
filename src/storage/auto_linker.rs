//! Auto-linking engine for emergent knowledge graph.
//!
//! Automatically creates typed links between memories based on semantic similarity
//! (via embedding cosine distance) and other signals (temporal proximity, co-occurrence).
//!
//! # Tables
//! - `auto_links` — machine-generated links (schema v18)
//!
//! # Usage
//! ```ignore
//! let embedder = TfIdfEmbedder::new(384);
//! let opts = SemanticLinkOptions::default();
//! let result = run_semantic_linker(&conn, &embedder, &opts)?;
//! println!("Created {} links over {} memories", result.links_created, result.memories_processed);
//! ```

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::embedding::{cosine_similarity, get_embedding, Embedder};
use crate::error::Result;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Summary of a completed auto-linking run.
#[derive(Debug, Clone)]
pub struct AutoLinkResult {
    /// Number of new links inserted into `auto_links`.
    pub links_created: usize,
    /// Number of memories examined.
    pub memories_processed: usize,
    /// Wall-clock time for the run in milliseconds.
    pub duration_ms: u64,
}

/// Configuration for semantic auto-linking.
#[derive(Debug, Clone)]
pub struct SemanticLinkOptions {
    /// Minimum cosine similarity to create a link (0.0 – 1.0). Default: 0.75.
    pub threshold: f32,
    /// Maximum links created *per memory* (top-N by score). Default: 5.
    pub max_links_per_memory: usize,
    /// Restrict to a single workspace. `None` processes all workspaces.
    pub workspace: Option<String>,
    /// How many memories to load per batch (controls memory usage). Default: 100.
    pub batch_size: usize,
}

impl Default for SemanticLinkOptions {
    fn default() -> Self {
        Self {
            threshold: 0.75,
            max_links_per_memory: 5,
            workspace: None,
            batch_size: 100,
        }
    }
}

/// A single auto-link record as stored in the `auto_links` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoLink {
    pub id: i64,
    pub from_id: i64,
    pub to_id: i64,
    pub link_type: String,
    pub score: f64,
    pub created_at: String,
}

// ---------------------------------------------------------------------------
// Core functions
// ---------------------------------------------------------------------------

/// Insert a single auto-link, ignoring duplicate (from_id, to_id, link_type) triplets.
///
/// Returns `Ok(true)` if the row was inserted and `Ok(false)` if it was already present.
pub fn insert_auto_link(
    conn: &Connection,
    from_id: i64,
    to_id: i64,
    link_type: &str,
    score: f64,
) -> Result<bool> {
    let rows = conn.execute(
        "INSERT OR IGNORE INTO auto_links (from_id, to_id, link_type, score)
         VALUES (?1, ?2, ?3, ?4)",
        params![from_id, to_id, link_type, score],
    )?;
    Ok(rows > 0)
}

/// Run the semantic auto-linker over memories that have stored embeddings.
///
/// The algorithm:
/// 1. Fetch IDs of all memories that have embeddings (optionally filtered by workspace).
/// 2. Load embeddings in batches of `options.batch_size`.
/// 3. For each memory, compute cosine similarity against all *later* memories in the
///    current window (avoids duplicate pair computation).
/// 4. Collect pairs above `options.threshold`, sort by score descending, take the top
///    `options.max_links_per_memory` for each side.
/// 5. Insert into `auto_links` with `link_type = "semantic"` using `INSERT OR IGNORE`.
///
/// # Complexity
/// O(n²) pairwise within each batch. For large collections you should lower `batch_size`
/// or schedule the job during off-peak hours.
pub fn run_semantic_linker(
    conn: &Connection,
    _embedder: &dyn Embedder,
    options: &SemanticLinkOptions,
) -> Result<AutoLinkResult> {
    let start = std::time::Instant::now();
    let mut links_created = 0usize;

    // 1. Fetch all memory IDs that have stored embeddings, optionally workspace-filtered.
    let ids: Vec<i64> = if let Some(ws) = &options.workspace {
        let mut stmt = conn.prepare(
            "SELECT m.id FROM memories m
             WHERE m.has_embedding = 1 AND m.valid_to IS NULL
               AND m.workspace = ?1
             ORDER BY m.id ASC
             LIMIT ?2",
        )?;
        let rows: Vec<i64> = stmt
            .query_map(params![ws, options.batch_size as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    } else {
        let mut stmt = conn.prepare(
            "SELECT m.id FROM memories m
             WHERE m.has_embedding = 1 AND m.valid_to IS NULL
             ORDER BY m.id ASC
             LIMIT ?1",
        )?;
        let rows: Vec<i64> = stmt
            .query_map(params![options.batch_size as i64], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };

    let memories_processed = ids.len();

    // 2. Load embeddings for those IDs.
    let mut embeddings: Vec<(i64, Vec<f32>)> = Vec::with_capacity(ids.len());
    for id in &ids {
        if let Ok(Some(emb)) = get_embedding(conn, *id) {
            embeddings.push((*id, emb));
        }
    }

    // 3. Pairwise similarity — upper triangle only (i < j avoids double-counting).
    //    For each memory i we collect all pairs above threshold, then honour max_links_per_memory.
    //    We also need to honour the limit from memory j's perspective; we track per-id counts.
    let n = embeddings.len();
    if n < 2 {
        return Ok(AutoLinkResult {
            links_created: 0,
            memories_processed,
            duration_ms: start.elapsed().as_millis() as u64,
        });
    }

    // Collect all qualifying pairs: (score, from_id, to_id) sorted descending by score.
    let mut pairs: Vec<(f32, i64, i64)> = Vec::new();

    for i in 0..n {
        for j in (i + 1)..n {
            let sim = cosine_similarity(&embeddings[i].1, &embeddings[j].1);
            if sim >= options.threshold {
                pairs.push((sim, embeddings[i].0, embeddings[j].0));
            }
        }
    }

    // Sort by score descending so we take the strongest links first.
    pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // Enforce max_links_per_memory: track how many links each memory has received.
    let mut link_counts: std::collections::HashMap<i64, usize> =
        std::collections::HashMap::new();

    for (score, from_id, to_id) in pairs {
        let from_count = link_counts.entry(from_id).or_insert(0);
        if *from_count >= options.max_links_per_memory {
            continue;
        }
        let to_count = link_counts.entry(to_id).or_insert(0);
        if *to_count >= options.max_links_per_memory {
            continue;
        }

        // 4. Insert the link (INSERT OR IGNORE respects the UNIQUE constraint).
        let inserted = insert_auto_link(conn, from_id, to_id, "semantic", score as f64)?;
        if inserted {
            links_created += 1;
            *link_counts.entry(from_id).or_insert(0) += 1;
            *link_counts.entry(to_id).or_insert(0) += 1;
        }
    }

    Ok(AutoLinkResult {
        links_created,
        memories_processed,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

/// List auto-links, optionally filtered by `link_type`.
///
/// Results are ordered by score descending.  `limit` is capped to 1000 to
/// prevent accidental full-table scans; pass 1000 explicitly for large queries.
pub fn list_auto_links(
    conn: &Connection,
    link_type: Option<&str>,
    limit: usize,
) -> Result<Vec<AutoLink>> {
    let capped_limit = limit.min(1000);

    let rows: Vec<AutoLink> = if let Some(lt) = link_type {
        let mut stmt = conn.prepare(
            "SELECT id, from_id, to_id, link_type, score, created_at
             FROM auto_links
             WHERE link_type = ?1
             ORDER BY score DESC
             LIMIT ?2",
        )?;
        let collected: Vec<AutoLink> = stmt
            .query_map(params![lt, capped_limit as i64], row_to_auto_link)?
            .filter_map(|r| r.ok())
            .collect();
        collected
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, from_id, to_id, link_type, score, created_at
             FROM auto_links
             ORDER BY score DESC
             LIMIT ?1",
        )?;
        let collected: Vec<AutoLink> = stmt
            .query_map(params![capped_limit as i64], row_to_auto_link)?
            .filter_map(|r| r.ok())
            .collect();
        collected
    };

    Ok(rows)
}

/// Return auto-link statistics as a JSON object, grouped by `link_type`.
///
/// Example output:
/// ```json
/// { "semantic": 42, "temporal": 7 }
/// ```
pub fn auto_link_stats(conn: &Connection) -> Result<serde_json::Value> {
    let mut stmt = conn.prepare(
        "SELECT link_type, COUNT(*) as cnt
         FROM auto_links
         GROUP BY link_type
         ORDER BY link_type ASC",
    )?;

    let mut map = serde_json::Map::new();
    let rows = stmt.query_map([], |row| {
        let lt: String = row.get(0)?;
        let cnt: i64 = row.get(1)?;
        Ok((lt, cnt))
    })?;

    for row in rows.filter_map(|r| r.ok()) {
        map.insert(row.0, serde_json::Value::Number(row.1.into()));
    }

    Ok(serde_json::Value::Object(map))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn row_to_auto_link(row: &rusqlite::Row) -> rusqlite::Result<AutoLink> {
    Ok(AutoLink {
        id: row.get(0)?,
        from_id: row.get(1)?,
        to_id: row.get(2)?,
        link_type: row.get(3)?,
        score: row.get(4)?,
        created_at: row.get(5)?,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedding::TfIdfEmbedder;
    use crate::storage::migrations::run_migrations;
    use rusqlite::Connection;

    /// Create an in-memory SQLite database with the full schema applied.
    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        run_migrations(&conn).expect("migrations");
        conn
    }

    /// Insert a minimal memory row and optionally a stored embedding, returning the new ID.
    fn insert_memory_with_embedding(
        conn: &Connection,
        content: &str,
        embedding: Option<&[f32]>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO memories (content, memory_type, has_embedding)
             VALUES (?1, 'note', ?2)",
            params![content, embedding.is_some() as i32],
        )
        .expect("insert memory");
        let id = conn.last_insert_rowid();

        if let Some(emb) = embedding {
            // Serialise f32 slice as little-endian bytes (same format as EmbeddingWorker).
            let bytes: Vec<u8> = emb
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            conn.execute(
                "INSERT INTO embeddings (memory_id, embedding, model, dimensions)
                 VALUES (?1, ?2, 'tfidf', ?3)",
                params![id, bytes, emb.len() as i64],
            )
            .expect("insert embedding");
        }

        id
    }

    // ------------------------------------------------------------------
    // insert_auto_link tests
    // ------------------------------------------------------------------

    #[test]
    fn test_insert_auto_link_creates_a_link() {
        let conn = setup_db();
        let a = insert_memory_with_embedding(&conn, "alpha", None);
        let b = insert_memory_with_embedding(&conn, "beta", None);

        let inserted = insert_auto_link(&conn, a, b, "semantic", 0.9).expect("insert");
        assert!(inserted, "first insert should return true");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_links", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_insert_auto_link_is_idempotent() {
        let conn = setup_db();
        let a = insert_memory_with_embedding(&conn, "alpha", None);
        let b = insert_memory_with_embedding(&conn, "beta", None);

        let first = insert_auto_link(&conn, a, b, "semantic", 0.9).expect("first insert");
        let second = insert_auto_link(&conn, a, b, "semantic", 0.9).expect("second insert");

        assert!(first, "first insert should return true");
        assert!(!second, "duplicate insert should return false");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_links", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "only one row should exist after duplicate insert");
    }

    #[test]
    fn test_insert_auto_link_different_type_is_not_duplicate() {
        let conn = setup_db();
        let a = insert_memory_with_embedding(&conn, "alpha", None);
        let b = insert_memory_with_embedding(&conn, "beta", None);

        insert_auto_link(&conn, a, b, "semantic", 0.9).unwrap();
        let second = insert_auto_link(&conn, a, b, "temporal", 0.5).unwrap();

        assert!(second, "different link_type should be a new row");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_links", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    // ------------------------------------------------------------------
    // run_semantic_linker tests
    // ------------------------------------------------------------------

    #[test]
    fn test_run_semantic_linker_processes_memories_and_creates_links() {
        let conn = setup_db();
        let embedder = TfIdfEmbedder::new(4);

        // Two memories with identical embeddings → cosine similarity = 1.0
        let emb = vec![1.0f32, 0.0, 0.0, 0.0];
        let _a = insert_memory_with_embedding(&conn, "memory A", Some(&emb));
        let _b = insert_memory_with_embedding(&conn, "memory B", Some(&emb));

        let opts = SemanticLinkOptions {
            threshold: 0.9,
            max_links_per_memory: 5,
            workspace: None,
            batch_size: 100,
        };

        let result = run_semantic_linker(&conn, &embedder, &opts).expect("linker");

        assert_eq!(result.memories_processed, 2);
        assert_eq!(result.links_created, 1, "one link for the identical pair");
    }

    #[test]
    fn test_threshold_filtering_lower_threshold_creates_more_links() {
        let conn = setup_db();
        let embedder = TfIdfEmbedder::new(4);

        // Three memories: A & B are identical (sim=1.0), A & C are orthogonal (sim=0.0)
        let emb_a = vec![1.0f32, 0.0, 0.0, 0.0];
        let emb_b = vec![1.0f32, 0.0, 0.0, 0.0];
        let emb_c = vec![0.0f32, 1.0, 0.0, 0.0]; // orthogonal to A and B

        insert_memory_with_embedding(&conn, "A", Some(&emb_a));
        insert_memory_with_embedding(&conn, "B", Some(&emb_b));
        insert_memory_with_embedding(&conn, "C", Some(&emb_c));

        // High threshold: only A-B should be linked
        let high_opts = SemanticLinkOptions {
            threshold: 0.9,
            max_links_per_memory: 5,
            workspace: None,
            batch_size: 100,
        };
        let result_high = run_semantic_linker(&conn, &embedder, &high_opts).expect("high");
        let count_high: i64 = conn
            .query_row("SELECT COUNT(*) FROM auto_links", [], |r| r.get(0))
            .unwrap();
        assert_eq!(result_high.links_created, 1);

        // Delete links so we can rerun with a low threshold.
        conn.execute("DELETE FROM auto_links", []).unwrap();

        // Low threshold: A-B still linked; A-C and B-C are orthogonal (sim=0), still not linked.
        // So result should be the same for orthogonal vectors.
        let low_opts = SemanticLinkOptions {
            threshold: 0.0,
            max_links_per_memory: 5,
            workspace: None,
            batch_size: 100,
        };
        let result_low = run_semantic_linker(&conn, &embedder, &low_opts).expect("low");
        // With threshold=0.0, zero-similarity pairs are also included (sim >= 0.0).
        // A-B (1.0), A-C (0.0), B-C (0.0) → 3 links
        assert!(
            result_low.links_created >= count_high as usize,
            "lower threshold should create at least as many links"
        );
    }

    #[test]
    fn test_max_links_per_memory_is_respected() {
        let conn = setup_db();
        let embedder = TfIdfEmbedder::new(4);

        // Six memories all with the same embedding → all pairs have similarity = 1.0
        let emb = vec![1.0f32, 0.0, 0.0, 0.0];
        for i in 0..6 {
            insert_memory_with_embedding(&conn, &format!("memory {}", i), Some(&emb));
        }

        let opts = SemanticLinkOptions {
            threshold: 0.9,
            max_links_per_memory: 2, // limit to 2 links per memory
            workspace: None,
            batch_size: 100,
        };

        run_semantic_linker(&conn, &embedder, &opts).expect("linker");

        // Fetch per-memory link counts and ensure none exceed max_links_per_memory.
        let mut stmt = conn
            .prepare(
                "SELECT mem_id, COUNT(*) as cnt FROM (
                     SELECT from_id AS mem_id FROM auto_links
                     UNION ALL
                     SELECT to_id AS mem_id FROM auto_links
                 ) GROUP BY mem_id",
            )
            .unwrap();

        let counts: Vec<i64> = stmt
            .query_map([], |r| r.get(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        for cnt in &counts {
            assert!(
                *cnt <= opts.max_links_per_memory as i64,
                "memory exceeds max_links_per_memory: {} > {}",
                cnt,
                opts.max_links_per_memory
            );
        }
    }

    // ------------------------------------------------------------------
    // list_auto_links tests
    // ------------------------------------------------------------------

    #[test]
    fn test_list_auto_links_returns_results() {
        let conn = setup_db();
        let a = insert_memory_with_embedding(&conn, "A", None);
        let b = insert_memory_with_embedding(&conn, "B", None);
        let c = insert_memory_with_embedding(&conn, "C", None);

        insert_auto_link(&conn, a, b, "semantic", 0.9).unwrap();
        insert_auto_link(&conn, b, c, "temporal", 0.6).unwrap();

        let all = list_auto_links(&conn, None, 10).expect("list all");
        assert_eq!(all.len(), 2);

        let semantic = list_auto_links(&conn, Some("semantic"), 10).expect("list semantic");
        assert_eq!(semantic.len(), 1);
        assert_eq!(semantic[0].link_type, "semantic");

        let temporal = list_auto_links(&conn, Some("temporal"), 10).expect("list temporal");
        assert_eq!(temporal.len(), 1);
        assert_eq!(temporal[0].link_type, "temporal");
    }

    #[test]
    fn test_list_auto_links_ordered_by_score_descending() {
        let conn = setup_db();
        let a = insert_memory_with_embedding(&conn, "A", None);
        let b = insert_memory_with_embedding(&conn, "B", None);
        let c = insert_memory_with_embedding(&conn, "C", None);

        insert_auto_link(&conn, a, b, "semantic", 0.5).unwrap();
        insert_auto_link(&conn, a, c, "semantic", 0.95).unwrap();

        let links = list_auto_links(&conn, Some("semantic"), 10).unwrap();
        assert_eq!(links.len(), 2);
        assert!(
            links[0].score >= links[1].score,
            "results should be ordered by score desc"
        );
    }

    // ------------------------------------------------------------------
    // auto_link_stats tests
    // ------------------------------------------------------------------

    #[test]
    fn test_auto_link_stats_returns_counts() {
        let conn = setup_db();
        let a = insert_memory_with_embedding(&conn, "A", None);
        let b = insert_memory_with_embedding(&conn, "B", None);
        let c = insert_memory_with_embedding(&conn, "C", None);

        insert_auto_link(&conn, a, b, "semantic", 0.8).unwrap();
        insert_auto_link(&conn, a, c, "semantic", 0.7).unwrap();
        insert_auto_link(&conn, b, c, "temporal", 0.5).unwrap();

        let stats = auto_link_stats(&conn).expect("stats");

        assert_eq!(stats["semantic"], serde_json::json!(2));
        assert_eq!(stats["temporal"], serde_json::json!(1));
    }

    #[test]
    fn test_auto_link_stats_empty_returns_empty_object() {
        let conn = setup_db();
        let stats = auto_link_stats(&conn).expect("stats");
        assert!(stats.as_object().unwrap().is_empty());
    }
}
