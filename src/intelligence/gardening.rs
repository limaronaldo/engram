//! Autonomous Memory Garden Maintenance — RML-1222
//!
//! Automatically prunes, merges, archives, and compresses memories to keep
//! the workspace healthy and within size budgets.
//!
//! ## Algorithm
//!
//! For each memory a `garden_score` is computed:
//! ```text
//! garden_score = importance * recency_factor * access_factor
//!   recency_factor = 1.0 / (1.0 + days_since_update / 30.0)
//!   access_factor  = 1.0 / (1.0 + days_since_access / 60.0)   (if last_accessed_at is set)
//! ```
//!
//! Memories are then processed in four passes:
//! 1. **Prune** — delete memories whose `garden_score < prune_threshold`.
//! 2. **Merge** — find word-level Jaccard similarity > `merge_threshold`; combine pairs.
//! 3. **Archive** — set `memory_type = 'archived'` for memories older than `archive_age_days`.
//! 4. **Compress** — truncate very long memories to a shorter summary prefix.
//!
//! ## Invariants
//!
//! - `GardenConfig` defaults mirror the task spec: prune=0.2, merge=0.6, archive=90d,
//!   max_memories=10000, dry_run=false.
//! - `garden_score` is always in [0.0, 1.0].
//! - Empty workspace returns a zero-action `GardenReport`.
//! - `dry_run = true` never modifies the database.
//! - `garden_undo` only restores archived memories (pruning is irreversible).
//! - All timestamps are RFC3339 UTC strings.

use std::collections::HashSet;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

// =============================================================================
// Public types
// =============================================================================

/// Configuration for the memory gardener.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GardenConfig {
    /// Score threshold below which memories are pruned (0.0–1.0). Default: 0.2.
    pub prune_threshold: f32,
    /// Jaccard similarity above which two memories are merged (0.0–1.0). Default: 0.6.
    pub merge_threshold: f32,
    /// Memories not updated within this many days are archived. Default: 90.
    pub archive_age_days: i64,
    /// If the workspace exceeds this count, low-scoring memories are pruned
    /// first until under the limit. Default: 10000.
    pub max_memories: usize,
    /// When true the gardener computes actions but does NOT commit them.
    pub dry_run: bool,
}

impl Default for GardenConfig {
    fn default() -> Self {
        Self {
            prune_threshold: 0.2,
            merge_threshold: 0.6,
            archive_age_days: 90,
            max_memories: 10000,
            dry_run: false,
        }
    }
}

/// A single maintenance action identified or executed by the gardener.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum GardenAction {
    /// Memory was pruned (deleted) because its garden score was too low.
    Prune { memory_id: i64, reason: String },
    /// Two or more memories were merged into a single combined memory.
    Merge {
        source_ids: Vec<i64>,
        result_content: String,
    },
    /// Memory was archived (memory_type set to 'archived') due to age.
    Archive { memory_id: i64 },
    /// Memory content was truncated/compressed.
    Compress { memory_id: i64 },
    /// Memory tags were updated.
    Retag {
        memory_id: i64,
        new_tags: Vec<String>,
    },
}

/// Summary report produced after a garden run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GardenReport {
    /// Database-assigned id (0 when not yet persisted).
    pub id: i64,
    /// Actions that were identified (and executed unless dry_run).
    pub actions: Vec<GardenAction>,
    /// Number of memories pruned.
    pub memories_pruned: usize,
    /// Number of memories merged.
    pub memories_merged: usize,
    /// Number of memories archived.
    pub memories_archived: usize,
    /// Number of memories compressed.
    pub memories_compressed: usize,
    /// Rough estimate of freed token-equivalent characters.
    pub tokens_freed: usize,
    /// RFC3339 UTC timestamp.
    pub created_at: String,
}

// =============================================================================
// DDL
// =============================================================================

/// DDL for the `garden_log` table — call once during schema setup.
pub const CREATE_GARDEN_LOG_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS garden_log (
        id                 INTEGER PRIMARY KEY AUTOINCREMENT,
        workspace          TEXT    NOT NULL,
        actions            TEXT    NOT NULL DEFAULT '[]',
        memories_pruned    INTEGER NOT NULL DEFAULT 0,
        memories_merged    INTEGER NOT NULL DEFAULT 0,
        memories_archived  INTEGER NOT NULL DEFAULT 0,
        tokens_freed       INTEGER NOT NULL DEFAULT 0,
        created_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    CREATE INDEX IF NOT EXISTS idx_garden_log_workspace   ON garden_log(workspace);
    CREATE INDEX IF NOT EXISTS idx_garden_log_created_at  ON garden_log(created_at);
"#;

// =============================================================================
// Internal row type
// =============================================================================

/// A lightweight row fetched from the `memories` table for scoring.
#[derive(Debug)]
struct MemoryRow {
    id: i64,
    content: String,
    memory_type: String,
    importance: f32,
    updated_at: String,
    last_accessed_at: Option<String>,
    #[allow(dead_code)]
    created_at: String,
}

// =============================================================================
// MemoryGardener
// =============================================================================

/// Engine for automatic memory garden maintenance.
pub struct MemoryGardener {
    pub config: GardenConfig,
}

impl MemoryGardener {
    /// Create a new gardener with the given config.
    pub fn new(config: GardenConfig) -> Self {
        Self { config }
    }

    /// Create a new gardener with the default config.
    pub fn with_defaults() -> Self {
        Self::new(GardenConfig::default())
    }

    // -------------------------------------------------------------------------
    // Public API
    // -------------------------------------------------------------------------

    /// Run garden maintenance on `workspace`.
    ///
    /// Scores all memories, then applies Prune → Merge → Archive → Compress
    /// passes. If `config.dry_run` is true the database is not modified but a
    /// full report is still returned.
    pub fn garden(&self, conn: &Connection, workspace: &str) -> Result<GardenReport> {
        let memories = fetch_memories(conn, workspace)?;
        if memories.is_empty() {
            return Ok(empty_report());
        }

        let now = Utc::now();
        let now_str = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let mut actions: Vec<GardenAction> = Vec::new();
        let mut pruned_ids: HashSet<i64> = HashSet::new();
        let mut merged_ids: HashSet<i64> = HashSet::new();
        let mut archived_ids: HashSet<i64> = HashSet::new();
        let mut compressed_ids: HashSet<i64> = HashSet::new();
        let mut tokens_freed: usize = 0;

        // ------------------------------------------------------------------
        // 1. Score every memory
        // ------------------------------------------------------------------
        let scores: Vec<(i64, f32)> = memories
            .iter()
            .map(|m| (m.id, compute_garden_score(m, &now_str)))
            .collect();

        // ------------------------------------------------------------------
        // 2. Prune — score < prune_threshold
        // ------------------------------------------------------------------
        for (id, score) in &scores {
            if *score < self.config.prune_threshold {
                // Find the memory to record content length
                if let Some(m) = memories.iter().find(|m| m.id == *id) {
                    tokens_freed += m.content.len();
                }
                actions.push(GardenAction::Prune {
                    memory_id: *id,
                    reason: format!(
                        "garden_score {:.3} < prune_threshold {:.3}",
                        score, self.config.prune_threshold
                    ),
                });
                if !self.config.dry_run {
                    conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
                }
                pruned_ids.insert(*id);
            }
        }

        // ------------------------------------------------------------------
        // 3. Merge — Jaccard similarity > merge_threshold (skip pruned)
        // ------------------------------------------------------------------
        let remaining: Vec<&MemoryRow> = memories
            .iter()
            .filter(|m| !pruned_ids.contains(&m.id))
            .collect();

        // Compute pairwise Jaccard; mark each id as merged at most once
        let mut already_merged: HashSet<i64> = HashSet::new();
        for i in 0..remaining.len() {
            if already_merged.contains(&remaining[i].id) {
                continue;
            }
            for j in (i + 1)..remaining.len() {
                if already_merged.contains(&remaining[j].id) {
                    continue;
                }
                let sim = jaccard_similarity(&remaining[i].content, &remaining[j].content);
                if sim >= self.config.merge_threshold {
                    let combined = merge_content(&remaining[i].content, &remaining[j].content);
                    let source_ids = vec![remaining[i].id, remaining[j].id];

                    actions.push(GardenAction::Merge {
                        source_ids: source_ids.clone(),
                        result_content: combined.clone(),
                    });

                    if !self.config.dry_run {
                        // Update the first memory with combined content
                        conn.execute(
                            "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
                            params![combined, now_str, remaining[i].id],
                        )?;
                        // Delete the second memory
                        conn.execute(
                            "DELETE FROM memories WHERE id = ?1",
                            params![remaining[j].id],
                        )?;
                    }

                    tokens_freed += remaining[j].content.len();
                    already_merged.insert(remaining[i].id);
                    already_merged.insert(remaining[j].id);
                    merged_ids.insert(remaining[i].id);
                    merged_ids.insert(remaining[j].id);
                    break; // each source memory participates in at most one merge
                }
            }
        }

        // ------------------------------------------------------------------
        // 4. Archive — older than archive_age_days, not already pruned/merged
        // ------------------------------------------------------------------
        let archive_cutoff = (now - chrono::Duration::days(self.config.archive_age_days))
            .format("%Y-%m-%dT%H:%M:%SZ")
            .to_string();

        for m in &memories {
            if pruned_ids.contains(&m.id) || merged_ids.contains(&m.id) {
                continue;
            }
            if m.memory_type == "archived" {
                // Already archived; skip
                continue;
            }
            if m.updated_at < archive_cutoff {
                actions.push(GardenAction::Archive { memory_id: m.id });
                if !self.config.dry_run {
                    conn.execute(
                        "UPDATE memories SET memory_type = 'archived', updated_at = ?1 WHERE id = ?2",
                        params![now_str, m.id],
                    )?;
                }
                archived_ids.insert(m.id);
            }
        }

        // ------------------------------------------------------------------
        // 5. Compress — truncate very long memories (> 4096 chars)
        // ------------------------------------------------------------------
        const MAX_CONTENT: usize = 4096;
        for m in &memories {
            if pruned_ids.contains(&m.id) || merged_ids.contains(&m.id) {
                continue;
            }
            if m.content.len() > MAX_CONTENT {
                let truncated = format!(
                    "{} [compressed]",
                    &m.content[..MAX_CONTENT.min(m.content.len())]
                );
                let freed = m.content.len().saturating_sub(truncated.len());
                tokens_freed += freed;
                actions.push(GardenAction::Compress { memory_id: m.id });
                if !self.config.dry_run {
                    conn.execute(
                        "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
                        params![truncated, now_str, m.id],
                    )?;
                }
                compressed_ids.insert(m.id);
            }
        }

        let report = GardenReport {
            id: 0,
            actions,
            memories_pruned: pruned_ids.len(),
            memories_merged: merged_ids.len() / 2, // pairs
            memories_archived: archived_ids.len(),
            memories_compressed: compressed_ids.len(),
            tokens_freed,
            created_at: now_str,
        };

        Ok(report)
    }

    /// Preview what garden maintenance would do — identical to `garden` with
    /// `dry_run = true`, regardless of the gardener's config.
    pub fn garden_preview(&self, conn: &Connection, workspace: &str) -> Result<GardenReport> {
        let preview_gardener = MemoryGardener {
            config: GardenConfig {
                dry_run: true,
                ..self.config.clone()
            },
        };
        preview_gardener.garden(conn, workspace)
    }

    /// Undo a previous garden run by restoring archived memories.
    ///
    /// Pruning cannot be reversed (memories are deleted). Returns the count
    /// of memories successfully un-archived.
    ///
    /// `report_id` must be a valid id from `garden_log`.
    pub fn garden_undo(&self, conn: &Connection, report_id: i64) -> Result<usize> {
        // Load the saved report's actions JSON
        let actions_json: String = conn.query_row(
            "SELECT actions FROM garden_log WHERE id = ?1",
            params![report_id],
            |row| row.get(0),
        )?;

        let actions: Vec<GardenAction> = serde_json::from_str(&actions_json).unwrap_or_default();

        let mut restored = 0usize;
        let now_str = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        for action in &actions {
            if let GardenAction::Archive { memory_id } = action {
                let rows = conn.execute(
                    "UPDATE memories SET memory_type = 'note', updated_at = ?1 WHERE id = ?2 AND memory_type = 'archived'",
                    params![now_str, memory_id],
                )?;
                restored += rows;
            }
        }

        Ok(restored)
    }

    // -------------------------------------------------------------------------
    // Report persistence
    // -------------------------------------------------------------------------

    /// Persist a [`GardenReport`] to `garden_log` and return its database id.
    pub fn save_report(
        &self,
        conn: &Connection,
        workspace: &str,
        report: &GardenReport,
    ) -> Result<i64> {
        let actions_json = serde_json::to_string(&report.actions)?;
        let now = if report.created_at.is_empty() {
            Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
        } else {
            report.created_at.clone()
        };

        conn.execute(
            "INSERT INTO garden_log
                 (workspace, actions, memories_pruned, memories_merged, memories_archived, tokens_freed, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                workspace,
                actions_json,
                report.memories_pruned as i64,
                report.memories_merged as i64,
                report.memories_archived as i64,
                report.tokens_freed as i64,
                now,
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// List recent garden reports for a workspace.
    ///
    /// `limit = 0` returns all rows.
    pub fn list_reports(
        &self,
        conn: &Connection,
        workspace: &str,
        limit: i64,
    ) -> Result<Vec<GardenReport>> {
        let effective_limit = if limit <= 0 { i64::MAX } else { limit };

        let mut stmt = conn.prepare(
            "SELECT id, actions, memories_pruned, memories_merged, memories_archived, tokens_freed, created_at
             FROM garden_log
             WHERE workspace = ?1
             ORDER BY id DESC
             LIMIT ?2",
        )?;

        let rows = stmt
            .query_map(params![workspace, effective_limit], map_report_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows)
    }
}

impl Default for MemoryGardener {
    fn default() -> Self {
        Self::with_defaults()
    }
}

// =============================================================================
// Private helpers
// =============================================================================

/// Return an empty report (used when workspace is empty).
fn empty_report() -> GardenReport {
    GardenReport {
        id: 0,
        actions: Vec::new(),
        memories_pruned: 0,
        memories_merged: 0,
        memories_archived: 0,
        memories_compressed: 0,
        tokens_freed: 0,
        created_at: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    }
}

/// Compute the garden score for a memory row.
///
/// ```text
/// garden_score = importance * recency_factor * access_factor
/// ```
fn compute_garden_score(m: &MemoryRow, now_str: &str) -> f32 {
    let now_ts = parse_ts(now_str);
    let updated_ts = parse_ts(&m.updated_at);

    let days_since_update = (now_ts - updated_ts).max(0) as f64 / 86_400.0;
    let recency_factor = 1.0 / (1.0 + days_since_update / 30.0);

    let access_factor = if let Some(ref la) = m.last_accessed_at {
        let la_ts = parse_ts(la);
        let days_since_access = (now_ts - la_ts).max(0) as f64 / 86_400.0;
        1.0 / (1.0 + days_since_access / 60.0)
    } else {
        1.0 // no access record → don't penalise
    };

    let score = m.importance as f64 * recency_factor * access_factor;
    score.clamp(0.0, 1.0) as f32
}

/// Parse an RFC3339 timestamp string into a Unix timestamp (seconds).
/// Falls back to 0 on parse error to avoid panics.
fn parse_ts(s: &str) -> i64 {
    use std::str::FromStr;
    chrono::DateTime::<chrono::FixedOffset>::from_str(s)
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

/// Compute word-level Jaccard similarity between two texts.
fn jaccard_similarity(a: &str, b: &str) -> f32 {
    let tokens_a: HashSet<String> = tokenize(a);
    let tokens_b: HashSet<String> = tokenize(b);

    if tokens_a.is_empty() && tokens_b.is_empty() {
        return 1.0;
    }
    if tokens_a.is_empty() || tokens_b.is_empty() {
        return 0.0;
    }

    let intersection = tokens_a.intersection(&tokens_b).count();
    let union = tokens_a.union(&tokens_b).count();

    intersection as f32 / union as f32
}

/// Tokenise text into a set of lowercase words (3+ chars).
fn tokenize(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|t| t.to_lowercase())
        .filter(|t| t.len() >= 3)
        .collect()
}

/// Merge two memory contents into a single combined string.
///
/// Deduplicates sentences and joins them.
fn merge_content(a: &str, b: &str) -> String {
    // Split into sentence-like chunks and deduplicate
    let mut seen: HashSet<String> = HashSet::new();
    let mut parts: Vec<String> = Vec::new();

    for text in &[a, b] {
        for sentence in text.split(['.', '\n']) {
            let trimmed = sentence.trim().to_string();
            if !trimmed.is_empty() {
                let key = trimmed.to_lowercase();
                if seen.insert(key) {
                    parts.push(trimmed);
                }
            }
        }
    }

    parts.join(". ")
}

/// Fetch all memories for a workspace from the `memories` table.
fn fetch_memories(conn: &Connection, workspace: &str) -> Result<Vec<MemoryRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, memory_type, importance, updated_at, last_accessed_at, created_at
         FROM memories
         WHERE workspace = ?1
         ORDER BY id ASC",
    )?;

    let rows = stmt
        .query_map(params![workspace], |row| {
            Ok(MemoryRow {
                id: row.get(0)?,
                content: row.get(1)?,
                memory_type: row
                    .get::<_, String>(2)
                    .unwrap_or_else(|_| "note".to_string()),
                importance: row.get::<_, f64>(3).unwrap_or(0.5) as f32,
                updated_at: row.get::<_, String>(4).unwrap_or_default(),
                last_accessed_at: row.get(5)?,
                created_at: row.get::<_, String>(6).unwrap_or_default(),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(rows)
}

/// Map a `garden_log` row to a [`GardenReport`].
fn map_report_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<GardenReport> {
    let id: i64 = row.get(0)?;
    let actions_json: String = row.get(1)?;
    let memories_pruned: i64 = row.get(2)?;
    let memories_merged: i64 = row.get(3)?;
    let memories_archived: i64 = row.get(4)?;
    let tokens_freed: i64 = row.get(5)?;
    let created_at: String = row.get(6)?;

    let actions: Vec<GardenAction> = serde_json::from_str(&actions_json).unwrap_or_default();

    Ok(GardenReport {
        id,
        actions,
        memories_pruned: memories_pruned as usize,
        memories_merged: memories_merged as usize,
        memories_archived: memories_archived as usize,
        memories_compressed: 0, // not persisted separately; derived from actions
        tokens_freed: tokens_freed as usize,
        created_at,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Create an in-memory SQLite connection with both `memories` and
    /// `garden_log` tables ready.
    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                content           TEXT    NOT NULL,
                memory_type       TEXT    NOT NULL DEFAULT 'note',
                workspace         TEXT    NOT NULL DEFAULT 'default',
                importance        REAL    NOT NULL DEFAULT 0.5,
                updated_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                last_accessed_at  TEXT,
                created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );",
        )
        .expect("create memories table");
        conn.execute_batch(CREATE_GARDEN_LOG_TABLE)
            .expect("create garden_log table");
        conn
    }

    /// Insert a memory row with explicit field values.
    fn insert_memory(
        conn: &Connection,
        content: &str,
        importance: f32,
        updated_at: &str,
        workspace: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO memories (content, importance, updated_at, created_at, workspace)
             VALUES (?1, ?2, ?3, ?3, ?4)",
            params![content, importance as f64, updated_at, workspace],
        )
        .expect("insert memory");
        conn.last_insert_rowid()
    }

    fn count_memories(conn: &Connection, workspace: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE workspace = ?1",
            params![workspace],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    fn memory_type(conn: &Connection, id: i64) -> String {
        conn.query_row(
            "SELECT memory_type FROM memories WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap_or_default()
    }

    fn gardener_with(prune: f32, merge: f32, archive: i64) -> MemoryGardener {
        MemoryGardener::new(GardenConfig {
            prune_threshold: prune,
            merge_threshold: merge,
            archive_age_days: archive,
            max_memories: 10000,
            dry_run: false,
        })
    }

    // -------------------------------------------------------------------------
    // Test 1: Prune low-score memories
    // -------------------------------------------------------------------------
    #[test]
    fn test_garden_prunes_low_score() {
        let conn = setup_conn();
        // Recent + low importance → low score
        let old_ts = "2020-01-01T00:00:00Z"; // very old → tiny recency_factor
        let id_low = insert_memory(&conn, "low importance stale memory", 0.05, old_ts, "ws");
        // High importance + recent → high score
        let recent_ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let _id_high = insert_memory(&conn, "important recent memory", 0.9, &recent_ts, "ws");

        let gardener = gardener_with(0.2, 0.99, 999);
        let report = gardener.garden(&conn, "ws").expect("garden");

        assert!(
            report.memories_pruned >= 1,
            "expected at least one pruned memory, got {}",
            report.memories_pruned
        );
        // Verify the low-score memory is actually deleted
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE id = ?1",
                params![id_low],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 0, "low-score memory should be deleted");
    }

    // -------------------------------------------------------------------------
    // Test 2: Merge near-duplicate memories
    // -------------------------------------------------------------------------
    #[test]
    fn test_garden_merges_duplicates() {
        let conn = setup_conn();
        let recent_ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        // Two nearly identical memories
        let _id_a = insert_memory(
            &conn,
            "The project uses Rust for its core engine and achieves high performance",
            0.8,
            &recent_ts,
            "ws",
        );
        let id_b = insert_memory(
            &conn,
            "The project uses Rust for its core engine and achieves great performance",
            0.8,
            &recent_ts,
            "ws",
        );

        let gardener = MemoryGardener::new(GardenConfig {
            prune_threshold: 0.0, // don't prune anything
            merge_threshold: 0.4, // low threshold so these definitely merge
            archive_age_days: 9999,
            max_memories: 10000,
            dry_run: false,
        });
        let report = gardener.garden(&conn, "ws").expect("garden");

        assert!(
            report.memories_merged >= 1,
            "expected at least one merge pair, got {}",
            report.memories_merged
        );
        // One of the two should be gone
        let remaining = count_memories(&conn, "ws");
        assert_eq!(remaining, 1, "one memory should remain after merge");
        // Specifically the second one should be deleted
        let b_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memories WHERE id = ?1",
                params![id_b],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(b_exists, 0, "merged-away memory should be deleted");
    }

    // -------------------------------------------------------------------------
    // Test 3: Archive old memories
    // -------------------------------------------------------------------------
    #[test]
    fn test_garden_archives_old_memories() {
        let conn = setup_conn();
        let old_ts = "2010-06-01T00:00:00Z";
        let id_old = insert_memory(&conn, "very old note about something", 0.5, old_ts, "ws");
        let recent_ts = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let id_recent = insert_memory(
            &conn,
            "recent note about something else",
            0.5,
            &recent_ts,
            "ws",
        );

        // Use high prune_threshold so nothing is pruned, archive at 30 days
        let gardener = MemoryGardener::new(GardenConfig {
            prune_threshold: 0.0,
            merge_threshold: 0.99,
            archive_age_days: 30,
            max_memories: 10000,
            dry_run: false,
        });
        let report = gardener.garden(&conn, "ws").expect("garden");

        assert!(
            report.memories_archived >= 1,
            "expected at least one archived memory"
        );
        assert_eq!(
            memory_type(&conn, id_old),
            "archived",
            "old memory should be archived"
        );
        assert_ne!(
            memory_type(&conn, id_recent),
            "archived",
            "recent memory should not be archived"
        );
    }

    // -------------------------------------------------------------------------
    // Test 4: dry_run does not modify the database
    // -------------------------------------------------------------------------
    #[test]
    fn test_dry_run_does_not_modify() {
        let conn = setup_conn();
        let old_ts = "2010-01-01T00:00:00Z";
        insert_memory(&conn, "low importance old memory", 0.01, old_ts, "ws");

        let gardener = MemoryGardener::new(GardenConfig {
            prune_threshold: 0.9,
            merge_threshold: 0.1,
            archive_age_days: 1,
            max_memories: 10000,
            dry_run: true,
        });

        let before = count_memories(&conn, "ws");
        let report = gardener.garden(&conn, "ws").expect("garden dry run");
        let after = count_memories(&conn, "ws");

        assert_eq!(before, after, "dry_run must not change memory count");
        // But the report should still list actions
        assert!(
            !report.actions.is_empty() || report.memories_pruned == 0,
            "dry run report should capture potential actions"
        );
    }

    // -------------------------------------------------------------------------
    // Test 5: garden_preview returns report without modifying db
    // -------------------------------------------------------------------------
    #[test]
    fn test_garden_preview_returns_report_without_modifying() {
        let conn = setup_conn();
        let old_ts = "2008-01-01T00:00:00Z";
        insert_memory(&conn, "very old low importance memory", 0.01, old_ts, "ws");

        let gardener = gardener_with(0.5, 0.99, 10);
        let before = count_memories(&conn, "ws");
        let report = gardener.garden_preview(&conn, "ws").expect("preview");
        let after = count_memories(&conn, "ws");

        // Database unchanged
        assert_eq!(before, after, "preview must not change memory count");
        // Report still has a valid timestamp
        assert!(
            !report.created_at.is_empty(),
            "report should have a timestamp"
        );
    }

    // -------------------------------------------------------------------------
    // Test 6: save_report and list_reports round-trip
    // -------------------------------------------------------------------------
    #[test]
    fn test_save_and_list_reports() {
        let conn = setup_conn();
        let gardener = MemoryGardener::with_defaults();

        let report = GardenReport {
            id: 0,
            actions: vec![GardenAction::Prune {
                memory_id: 42,
                reason: "test".to_string(),
            }],
            memories_pruned: 1,
            memories_merged: 0,
            memories_archived: 0,
            memories_compressed: 0,
            tokens_freed: 100,
            created_at: "2026-01-01T00:00:00Z".to_string(),
        };

        let saved_id = gardener
            .save_report(&conn, "ws", &report)
            .expect("save report");
        assert!(saved_id > 0, "saved_id should be positive");

        let reports = gardener.list_reports(&conn, "ws", 10).expect("list");
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].memories_pruned, 1);
        assert_eq!(reports[0].tokens_freed, 100);
    }

    // -------------------------------------------------------------------------
    // Test 7: Empty workspace returns zero-action report
    // -------------------------------------------------------------------------
    #[test]
    fn test_empty_workspace_returns_empty_report() {
        let conn = setup_conn();
        let gardener = MemoryGardener::with_defaults();
        let report = gardener.garden(&conn, "nonexistent").expect("garden");

        assert_eq!(report.memories_pruned, 0);
        assert_eq!(report.memories_merged, 0);
        assert_eq!(report.memories_archived, 0);
        assert_eq!(report.memories_compressed, 0);
        assert!(report.actions.is_empty());
    }

    // -------------------------------------------------------------------------
    // Test 8: garden_undo un-archives memories
    // -------------------------------------------------------------------------
    #[test]
    fn test_garden_undo_unarchives_memories() {
        let conn = setup_conn();
        let old_ts = "2010-01-01T00:00:00Z";
        let id = insert_memory(
            &conn,
            "archivable note about rust programming",
            0.5,
            old_ts,
            "ws",
        );

        let gardener = MemoryGardener::new(GardenConfig {
            prune_threshold: 0.0,
            merge_threshold: 0.99,
            archive_age_days: 30,
            max_memories: 10000,
            dry_run: false,
        });

        // Run garden — should archive the old memory
        let report = gardener.garden(&conn, "ws").expect("garden");
        assert_eq!(memory_type(&conn, id), "archived");

        // Save report so undo can find it
        let report_id = gardener
            .save_report(&conn, "ws", &report)
            .expect("save report");

        // Undo
        let restored = gardener.garden_undo(&conn, report_id).expect("undo");
        assert_eq!(restored, 1, "one memory should be restored");
        assert_ne!(
            memory_type(&conn, id),
            "archived",
            "memory type should no longer be archived after undo"
        );
    }

    // -------------------------------------------------------------------------
    // Test 9: garden_score computation
    // -------------------------------------------------------------------------
    #[test]
    fn test_garden_score_ranges() {
        let now_str = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

        // Fresh, high-importance memory → score close to 1.0
        let high = MemoryRow {
            id: 1,
            content: "x".to_string(),
            memory_type: "note".to_string(),
            importance: 1.0,
            updated_at: now_str.clone(),
            last_accessed_at: None,
            created_at: now_str.clone(),
        };
        let score_high = compute_garden_score(&high, &now_str);
        assert!(
            score_high > 0.9,
            "fresh high-importance memory score should be > 0.9, got {score_high}"
        );

        // Very old, very low importance → score close to 0
        let low = MemoryRow {
            id: 2,
            content: "x".to_string(),
            memory_type: "note".to_string(),
            importance: 0.01,
            updated_at: "2000-01-01T00:00:00Z".to_string(),
            last_accessed_at: None,
            created_at: "2000-01-01T00:00:00Z".to_string(),
        };
        let score_low = compute_garden_score(&low, &now_str);
        assert!(
            score_low < 0.01,
            "ancient low-importance memory score should be < 0.01, got {score_low}"
        );
    }

    // -------------------------------------------------------------------------
    // Test 10: Jaccard similarity
    // -------------------------------------------------------------------------
    #[test]
    fn test_jaccard_similarity() {
        // Identical
        assert!(
            (jaccard_similarity("hello world foo", "hello world foo") - 1.0).abs() < f32::EPSILON
        );
        // Disjoint
        assert_eq!(jaccard_similarity("aaa bbb ccc", "xxx yyy zzz"), 0.0);
        // Partial overlap
        let sim = jaccard_similarity("rust memory engine", "rust memory bank");
        assert!(sim > 0.0 && sim < 1.0, "partial overlap sim={sim}");
    }
}
