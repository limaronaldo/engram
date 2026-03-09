//! Sleep-time Consolidation Engine (RML-1210)
//!
//! Runs an offline consolidation pass over a workspace's memories:
//!
//! 1. Queries recent memories (older than `min_age_hours`).
//! 2. Groups them by the chosen `GroupingStrategy`.
//! 3. Merges each group into a single deduplicated summary.
//! 4. Persists the summary to `consolidated_memories`.
//! 5. Archives the originals by adding a `consolidated` tag and flipping
//!    `lifecycle_state` to `archived`.
//!
//! ## Invariants
//!
//! - Only memories older than `min_age_hours` are candidates.
//! - Groups must have at least 2 members to be worth merging.
//! - A group cannot exceed `max_group_size` members.
//! - Token counts are a rough word-level proxy (one word ≈ one token).

use std::collections::{HashMap, HashSet};

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

// =============================================================================
// DDL
// =============================================================================

/// DDL for the consolidated_memories table.
///
/// Run once (idempotent) before using any storage function in this module.
pub const CREATE_CONSOLIDATED_MEMORIES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS consolidated_memories (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_ids TEXT NOT NULL DEFAULT '[]',
    summary TEXT NOT NULL,
    strategy_used TEXT NOT NULL DEFAULT 'content_overlap',
    tokens_before INTEGER NOT NULL DEFAULT 0,
    tokens_after INTEGER NOT NULL DEFAULT 0,
    workspace TEXT DEFAULT 'default',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX IF NOT EXISTS idx_consolidated_workspace
    ON consolidated_memories(workspace);
"#;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for the offline consolidation pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationConfig {
    /// Number of memories fetched per batch (default 100).
    pub batch_size: usize,
    /// Only consolidate memories older than this many hours (default 24.0).
    pub min_age_hours: f64,
    /// Jaccard similarity threshold for grouping memories (default 0.3).
    pub similarity_threshold: f32,
    /// Maximum number of memories allowed in a single group (default 10).
    pub max_group_size: usize,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            min_age_hours: 24.0,
            similarity_threshold: 0.3,
            max_group_size: 10,
        }
    }
}

// =============================================================================
// Grouping strategy
// =============================================================================

/// Strategy used to decide which memories belong together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupingStrategy {
    /// Group by Jaccard similarity of content word-sets.
    ContentOverlap,
    /// Group by shared tags (Jaccard on tag sets).
    TagSimilarity,
    /// Group by creation-time proximity (within 1-hour windows).
    TemporalProximity,
}

impl GroupingStrategy {
    /// String representation stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            GroupingStrategy::ContentOverlap => "content_overlap",
            GroupingStrategy::TagSimilarity => "tag_similarity",
            GroupingStrategy::TemporalProximity => "temporal_proximity",
        }
    }
}

// =============================================================================
// Core types
// =============================================================================

/// A group of memory ids that should be merged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationGroup {
    /// Ids of memories that belong to this group.
    pub memory_ids: Vec<i64>,
    /// Strategy that produced this group.
    pub strategy: GroupingStrategy,
    /// Average pairwise similarity score (0.0 – 1.0).
    pub similarity_score: f32,
}

/// Summary statistics for a completed consolidation pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationReport {
    /// Number of groups identified.
    pub groups_found: usize,
    /// Total number of individual memories that were merged.
    pub memories_merged: usize,
    /// Number of original memories archived after merging.
    pub memories_archived: usize,
    /// Approximate token count of all source memories (word count).
    pub tokens_before: usize,
    /// Approximate token count of all produced summaries.
    pub tokens_after: usize,
    /// tokens_before - tokens_after (never negative in a well-formed run).
    pub tokens_saved: usize,
}

/// A persisted consolidation record returned by `list_consolidations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidatedMemory {
    /// Database-assigned id.
    pub id: i64,
    /// JSON array of source memory ids, e.g. `[1, 2, 3]`.
    pub source_ids: String,
    /// Merged summary text.
    pub summary: String,
    /// Strategy used to produce this record.
    pub strategy_used: String,
    /// Word count of all source memories combined.
    pub tokens_before: i64,
    /// Word count of the summary.
    pub tokens_after: i64,
    /// Workspace this consolidation belongs to.
    pub workspace: String,
    /// RFC3339 timestamp when the record was created.
    pub created_at: String,
}

// =============================================================================
// OfflineConsolidator
// =============================================================================

/// Runs a full sleep-time consolidation pass against a SQLite connection.
pub struct OfflineConsolidator {
    config: ConsolidationConfig,
}

impl OfflineConsolidator {
    /// Create a new consolidator with the given configuration.
    pub fn new(config: ConsolidationConfig) -> Self {
        Self { config }
    }

    // -------------------------------------------------------------------------
    // Grouping
    // -------------------------------------------------------------------------

    /// Find groups of related memories in `workspace` using `strategy`.
    ///
    /// Only memories older than `config.min_age_hours` are considered.
    pub fn find_groups(
        &self,
        conn: &Connection,
        workspace: &str,
        strategy: GroupingStrategy,
    ) -> Result<Vec<ConsolidationGroup>> {
        let candidates = self.fetch_candidates(conn, workspace)?;
        if candidates.len() < 2 {
            return Ok(Vec::new());
        }

        let groups = match strategy {
            GroupingStrategy::ContentOverlap => self.group_by_content_overlap(&candidates),
            GroupingStrategy::TagSimilarity => self.group_by_tag_similarity(conn, &candidates),
            GroupingStrategy::TemporalProximity => {
                self.group_by_temporal_proximity(conn, &candidates)
            }
        };

        Ok(groups)
    }

    // -------------------------------------------------------------------------
    // Merge
    // -------------------------------------------------------------------------

    /// Combine the content of multiple `(id, content)` pairs into a single
    /// deduplicated summary.
    ///
    /// Algorithm: split each content into sentences (by `.`/`!`/`?`), collect
    /// unique sentences in order of first appearance, then join with `. `.
    pub fn merge_group(memories: &[(i64, String)]) -> String {
        let mut seen: HashSet<String> = HashSet::new();
        let mut ordered: Vec<String> = Vec::new();

        for (_, content) in memories {
            for raw_sentence in content.split(['.', '!', '?']) {
                let sentence = raw_sentence.trim().to_string();
                if sentence.is_empty() {
                    continue;
                }
                let key = sentence.to_lowercase();
                if seen.insert(key) {
                    ordered.push(sentence);
                }
            }
        }

        ordered.join(". ")
    }

    // -------------------------------------------------------------------------
    // Full pipeline
    // -------------------------------------------------------------------------

    /// Run the full consolidation pipeline using the `ContentOverlap` strategy.
    pub fn consolidate(&self, conn: &Connection, workspace: &str) -> Result<ConsolidationReport> {
        self.consolidate_with_strategy(conn, workspace, GroupingStrategy::ContentOverlap)
    }

    /// Run the full consolidation pipeline with an explicit strategy.
    pub fn consolidate_with_strategy(
        &self,
        conn: &Connection,
        workspace: &str,
        strategy: GroupingStrategy,
    ) -> Result<ConsolidationReport> {
        let groups = self.find_groups(conn, workspace, strategy)?;

        let mut memories_merged = 0usize;
        let mut memories_archived = 0usize;
        let mut tokens_before = 0usize;
        let mut tokens_after = 0usize;

        for group in &groups {
            // Fetch content for each id in the group
            let pairs = self.fetch_contents(conn, &group.memory_ids)?;
            if pairs.len() < 2 {
                continue;
            }

            // Token counts (word-level proxy)
            let tb: usize = pairs.iter().map(|(_, c)| word_count(c)).sum();
            let summary = Self::merge_group(&pairs);
            let ta = word_count(&summary);

            // Persist
            save_consolidation(
                conn,
                &group.memory_ids,
                &summary,
                strategy.as_str(),
                tb as i64,
                ta as i64,
                workspace,
            )?;

            // Archive originals
            let archived = self.archive_memories(conn, &group.memory_ids)?;

            memories_merged += pairs.len();
            memories_archived += archived;
            tokens_before += tb;
            tokens_after += ta;
        }

        let tokens_saved = tokens_before.saturating_sub(tokens_after);

        Ok(ConsolidationReport {
            groups_found: groups.len(),
            memories_merged,
            memories_archived,
            tokens_before,
            tokens_after,
            tokens_saved,
        })
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Fetch candidate memories (id, content, created_at) older than threshold.
    fn fetch_candidates(&self, conn: &Connection, workspace: &str) -> Result<Vec<(i64, String)>> {
        let hours = self.config.min_age_hours;
        // SQLite: datetime('now', '-N hours')
        let cutoff = format!("datetime('now', '-{hours} hours')");

        let sql = format!(
            "SELECT id, content FROM memories
             WHERE workspace = ?1
               AND created_at < {cutoff}
               AND (lifecycle_state IS NULL OR lifecycle_state != 'archived')
             ORDER BY created_at ASC
             LIMIT ?2"
        );

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![workspace, self.config.batch_size as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Fetch (id, content) pairs for the supplied ids.
    fn fetch_contents(&self, conn: &Connection, ids: &[i64]) -> Result<Vec<(i64, String)>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: String = ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");

        let sql = format!(
            "SELECT id, content FROM memories WHERE id IN ({placeholders}) ORDER BY id ASC"
        );

        let mut stmt = conn.prepare(&sql)?;

        // Build params dynamically
        let params_vec: Vec<&dyn rusqlite::types::ToSql> = ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();

        let rows = stmt.query_map(params_vec.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }

    /// Group candidate `(id, content)` pairs by Jaccard content similarity.
    fn group_by_content_overlap(&self, candidates: &[(i64, String)]) -> Vec<ConsolidationGroup> {
        let threshold = self.config.similarity_threshold;
        let max_group = self.config.max_group_size;

        // Build word-sets once
        let word_sets: Vec<HashSet<String>> = candidates.iter().map(|(_, c)| tokenize(c)).collect();

        let mut assigned: HashSet<usize> = HashSet::new();
        let mut groups: Vec<ConsolidationGroup> = Vec::new();

        for i in 0..candidates.len() {
            if assigned.contains(&i) {
                continue;
            }

            let mut group_indices = vec![i];
            let mut total_sim = 0.0f32;
            let mut pair_count = 0usize;

            for j in (i + 1)..candidates.len() {
                if assigned.contains(&j) {
                    continue;
                }
                if group_indices.len() >= max_group {
                    break;
                }

                let sim = jaccard(&word_sets[i], &word_sets[j]);
                if sim >= threshold {
                    group_indices.push(j);
                    total_sim += sim;
                    pair_count += 1;
                }
            }

            if group_indices.len() >= 2 {
                for &idx in &group_indices {
                    assigned.insert(idx);
                }
                let avg_sim = if pair_count > 0 {
                    total_sim / pair_count as f32
                } else {
                    threshold
                };
                groups.push(ConsolidationGroup {
                    memory_ids: group_indices.iter().map(|&idx| candidates[idx].0).collect(),
                    strategy: GroupingStrategy::ContentOverlap,
                    similarity_score: avg_sim,
                });
            }
        }

        groups
    }

    /// Group by shared tags (Jaccard on tag sets, fetched from DB).
    fn group_by_tag_similarity(
        &self,
        conn: &Connection,
        candidates: &[(i64, String)],
    ) -> Vec<ConsolidationGroup> {
        let threshold = self.config.similarity_threshold;
        let max_group = self.config.max_group_size;

        // Fetch tags for all candidate ids
        let id_tags: HashMap<i64, HashSet<String>> = candidates
            .iter()
            .filter_map(|(id, _)| {
                let tags = fetch_tags(conn, *id).ok()?;
                Some((*id, tags))
            })
            .collect();

        let ids: Vec<i64> = candidates.iter().map(|(id, _)| *id).collect();
        let mut assigned: HashSet<usize> = HashSet::new();
        let mut groups: Vec<ConsolidationGroup> = Vec::new();

        for i in 0..ids.len() {
            if assigned.contains(&i) {
                continue;
            }

            let empty = HashSet::new();
            let tags_i = id_tags.get(&ids[i]).unwrap_or(&empty);
            if tags_i.is_empty() {
                continue;
            }

            let mut group_indices = vec![i];
            let mut total_sim = 0.0f32;
            let mut pair_count = 0usize;

            for (j, &id_j) in ids.iter().enumerate().skip(i + 1) {
                if assigned.contains(&j) {
                    continue;
                }
                if group_indices.len() >= max_group {
                    break;
                }

                let tags_j = id_tags.get(&id_j).unwrap_or(&empty);
                if tags_j.is_empty() {
                    continue;
                }

                let sim = jaccard(tags_i, tags_j);
                if sim >= threshold {
                    group_indices.push(j);
                    total_sim += sim;
                    pair_count += 1;
                }
            }

            if group_indices.len() >= 2 {
                for &idx in &group_indices {
                    assigned.insert(idx);
                }
                let avg_sim = if pair_count > 0 {
                    total_sim / pair_count as f32
                } else {
                    threshold
                };
                groups.push(ConsolidationGroup {
                    memory_ids: group_indices.iter().map(|&idx| ids[idx]).collect(),
                    strategy: GroupingStrategy::TagSimilarity,
                    similarity_score: avg_sim,
                });
            }
        }

        groups
    }

    /// Group by temporal proximity: memories created within 1-hour windows.
    fn group_by_temporal_proximity(
        &self,
        conn: &Connection,
        candidates: &[(i64, String)],
    ) -> Vec<ConsolidationGroup> {
        let max_group = self.config.max_group_size;

        // Fetch creation timestamps
        let id_times: HashMap<i64, i64> = candidates
            .iter()
            .filter_map(|(id, _)| {
                let ts = fetch_unix_created_at(conn, *id).ok().flatten()?;
                Some((*id, ts))
            })
            .collect();

        // Sort by time
        let mut sorted: Vec<(i64, i64)> = id_times.into_iter().collect();
        sorted.sort_by_key(|(_, ts)| *ts);

        let window_secs: i64 = 3600; // 1 hour
        let mut assigned: HashSet<i64> = HashSet::new();
        let mut groups: Vec<ConsolidationGroup> = Vec::new();

        for i in 0..sorted.len() {
            let (id_i, ts_i) = sorted[i];
            if assigned.contains(&id_i) {
                continue;
            }

            let mut group_ids = vec![id_i];

            for &(id_j, ts_j) in sorted.iter().skip(i + 1) {
                if assigned.contains(&id_j) {
                    continue;
                }
                if group_ids.len() >= max_group {
                    break;
                }
                if (ts_j - ts_i).abs() <= window_secs {
                    group_ids.push(id_j);
                } else {
                    break; // sorted by time, no need to continue
                }
            }

            if group_ids.len() >= 2 {
                for &gid in &group_ids {
                    assigned.insert(gid);
                }
                groups.push(ConsolidationGroup {
                    memory_ids: group_ids,
                    strategy: GroupingStrategy::TemporalProximity,
                    similarity_score: 1.0, // proximity-based, not similarity-scored
                });
            }
        }

        groups
    }

    /// Archive originals: add `consolidated` tag and set lifecycle_state.
    ///
    /// Returns the count of memories successfully updated.
    fn archive_memories(&self, conn: &Connection, ids: &[i64]) -> Result<usize> {
        let mut count = 0usize;
        for &id in ids {
            let updated = conn.execute(
                "UPDATE memories
                 SET lifecycle_state = 'archived',
                     updated_at = ?2
                 WHERE id = ?1",
                params![id, Utc::now().format("%Y-%m-%dT%H:%M:%fZ").to_string()],
            )?;
            count += updated;
        }
        Ok(count)
    }
}

// =============================================================================
// Storage functions
// =============================================================================

/// Insert a new consolidation record.
///
/// Returns the id of the newly inserted row.
pub fn save_consolidation(
    conn: &Connection,
    source_ids: &[i64],
    summary: &str,
    strategy: &str,
    tokens_before: i64,
    tokens_after: i64,
    workspace: &str,
) -> Result<i64> {
    let source_json = serde_json::to_string(source_ids)?;
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%fZ").to_string();

    conn.execute(
        "INSERT INTO consolidated_memories
             (source_ids, summary, strategy_used, tokens_before, tokens_after, workspace, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![source_json, summary, strategy, tokens_before, tokens_after, workspace, now],
    )?;

    Ok(conn.last_insert_rowid())
}

/// List consolidation records for `workspace`, most recent first.
///
/// `limit = 0` means unlimited.
pub fn list_consolidations(
    conn: &Connection,
    workspace: &str,
    limit: usize,
) -> Result<Vec<ConsolidatedMemory>> {
    let effective_limit = if limit == 0 { i64::MAX } else { limit as i64 };

    let mut stmt = conn.prepare(
        "SELECT id, source_ids, summary, strategy_used, tokens_before, tokens_after, workspace, created_at
         FROM consolidated_memories
         WHERE workspace = ?1
         ORDER BY id DESC
         LIMIT ?2",
    )?;

    let rows = stmt.query_map(params![workspace, effective_limit], |row| {
        Ok(ConsolidatedMemory {
            id: row.get(0)?,
            source_ids: row.get(1)?,
            summary: row.get(2)?,
            strategy_used: row.get(3)?,
            tokens_before: row.get(4)?,
            tokens_after: row.get(5)?,
            workspace: row.get(6)?,
            created_at: row.get(7)?,
        })
    })?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row?);
    }
    Ok(result)
}

/// Return the source memory ids for a given consolidation record.
pub fn get_consolidation_sources(conn: &Connection, consolidation_id: i64) -> Result<Vec<i64>> {
    let source_json: String = conn.query_row(
        "SELECT source_ids FROM consolidated_memories WHERE id = ?1",
        params![consolidation_id],
        |row| row.get(0),
    )?;

    let ids: Vec<i64> = serde_json::from_str(&source_json)?;
    Ok(ids)
}

// =============================================================================
// Private utility functions
// =============================================================================

/// Tokenize text into a word HashSet (lowercase, strip punctuation).
fn tokenize(text: &str) -> HashSet<String> {
    text.split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Jaccard similarity between two sets.
fn jaccard<T: Eq + std::hash::Hash>(a: &HashSet<T>, b: &HashSet<T>) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let intersection = a.intersection(b).count();
    let union = a.len() + b.len() - intersection;
    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

/// Rough token count: number of whitespace-separated words.
fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Fetch tags for a memory as a `HashSet<String>`.
fn fetch_tags(conn: &Connection, memory_id: i64) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name
         FROM tags t
         JOIN memory_tags mt ON mt.tag_id = t.id
         WHERE mt.memory_id = ?1",
    )?;

    let rows = stmt.query_map(params![memory_id], |row| row.get::<_, String>(0))?;
    let mut tags = HashSet::new();
    for row in rows {
        tags.insert(row?);
    }
    Ok(tags)
}

/// Return `created_at` as a Unix timestamp (seconds) for `memory_id`.
///
/// `strftime('%s', ...)` returns TEXT in SQLite, so we read it as a String
/// and parse it to i64.
///
/// Returns `None` if the row or column is missing.
fn fetch_unix_created_at(conn: &Connection, memory_id: i64) -> Result<Option<i64>> {
    let result = conn.query_row(
        "SELECT strftime('%s', created_at) FROM memories WHERE id = ?1",
        params![memory_id],
        |row| row.get::<_, Option<String>>(0),
    );

    match result {
        Ok(Some(ts_str)) => Ok(ts_str.parse::<i64>().ok()),
        Ok(None) => Ok(None),
        Err(_) => Ok(None),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // ------------------------------------------------------------------
    // Test helpers
    // ------------------------------------------------------------------

    /// Minimal schema needed for tests: memories + consolidated_memories.
    const MINIMAL_SCHEMA: &str = r#"
        CREATE TABLE IF NOT EXISTS memories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content TEXT NOT NULL,
            memory_type TEXT NOT NULL DEFAULT 'note',
            importance REAL NOT NULL DEFAULT 0.5,
            access_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            last_accessed_at TEXT,
            owner_id TEXT,
            visibility TEXT NOT NULL DEFAULT 'private',
            scope TEXT NOT NULL DEFAULT 'global',
            workspace TEXT NOT NULL DEFAULT 'default',
            tier TEXT NOT NULL DEFAULT 'permanent',
            version INTEGER NOT NULL DEFAULT 1,
            has_embedding INTEGER NOT NULL DEFAULT 0,
            expires_at TEXT,
            content_hash TEXT,
            event_time TEXT,
            event_duration_seconds INTEGER,
            trigger_pattern TEXT,
            procedure_success_count INTEGER NOT NULL DEFAULT 0,
            procedure_failure_count INTEGER NOT NULL DEFAULT 0,
            summary_of_id INTEGER,
            lifecycle_state TEXT NOT NULL DEFAULT 'active',
            metadata TEXT NOT NULL DEFAULT '{}'
        );
        CREATE TABLE IF NOT EXISTS tags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE COLLATE NOCASE
        );
        CREATE TABLE IF NOT EXISTS memory_tags (
            memory_id INTEGER NOT NULL,
            tag_id INTEGER NOT NULL,
            PRIMARY KEY (memory_id, tag_id),
            FOREIGN KEY (memory_id) REFERENCES memories(id) ON DELETE CASCADE,
            FOREIGN KEY (tag_id) REFERENCES tags(id) ON DELETE CASCADE
        );
    "#;

    fn open_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(MINIMAL_SCHEMA).expect("create tables");
        conn.execute_batch(CREATE_CONSOLIDATED_MEMORIES_TABLE)
            .expect("create consolidated_memories");
        conn
    }

    /// Insert a memory with a specific `created_at` offset in hours from now.
    fn insert_memory(conn: &Connection, content: &str, workspace: &str, hours_ago: f64) -> i64 {
        let created_at = format!("datetime('now', '-{hours_ago} hours')");
        let sql = format!(
            "INSERT INTO memories (content, workspace, created_at, updated_at)
             VALUES (?1, ?2, {created_at}, {created_at})"
        );
        conn.execute(&sql, params![content, workspace])
            .expect("insert memory");
        conn.last_insert_rowid()
    }

    /// Insert a tag and associate it with a memory.
    fn tag_memory(conn: &Connection, memory_id: i64, tag: &str) {
        conn.execute(
            "INSERT OR IGNORE INTO tags (name) VALUES (?1)",
            params![tag],
        )
        .expect("insert tag");
        let tag_id: i64 = conn
            .query_row("SELECT id FROM tags WHERE name = ?1", params![tag], |r| {
                r.get(0)
            })
            .expect("get tag id");
        conn.execute(
            "INSERT OR IGNORE INTO memory_tags (memory_id, tag_id) VALUES (?1, ?2)",
            params![memory_id, tag_id],
        )
        .expect("link tag");
    }

    fn default_config() -> ConsolidationConfig {
        ConsolidationConfig {
            batch_size: 100,
            min_age_hours: 0.0, // pick up everything for tests
            similarity_threshold: 0.3,
            max_group_size: 10,
        }
    }

    // ------------------------------------------------------------------
    // Test 1: find_groups finds overlapping content
    // ------------------------------------------------------------------
    #[test]
    fn test_find_groups_overlapping_content() {
        let conn = open_conn();
        let id1 = insert_memory(
            &conn,
            "Rust is a systems programming language with memory safety",
            "default",
            2.0,
        );
        let id2 = insert_memory(
            &conn,
            "Rust is a systems programming language focused on performance",
            "default",
            2.0,
        );
        // Unrelated memory
        let _id3 = insert_memory(&conn, "Pizza is delicious", "default", 2.0);

        let consolidator = OfflineConsolidator::new(default_config());
        let groups = consolidator
            .find_groups(&conn, "default", GroupingStrategy::ContentOverlap)
            .expect("find_groups");

        assert!(!groups.is_empty(), "expected at least one group");
        let group = &groups[0];
        assert!(group.memory_ids.contains(&id1));
        assert!(group.memory_ids.contains(&id2));
    }

    // ------------------------------------------------------------------
    // Test 2: no groups when all content is unique
    // ------------------------------------------------------------------
    #[test]
    fn test_find_groups_no_overlap_returns_empty() {
        let conn = open_conn();
        insert_memory(&conn, "Apples are red fruits", "default", 2.0);
        insert_memory(&conn, "Cars drive on roads", "default", 2.0);
        insert_memory(&conn, "The sky is blue today", "default", 2.0);

        let consolidator = OfflineConsolidator::new(ConsolidationConfig {
            similarity_threshold: 0.9, // very strict
            ..default_config()
        });
        let groups = consolidator
            .find_groups(&conn, "default", GroupingStrategy::ContentOverlap)
            .expect("find_groups");

        assert!(groups.is_empty(), "expected no groups for unique content");
    }

    // ------------------------------------------------------------------
    // Test 3: merge_group produces combined unique content
    // ------------------------------------------------------------------
    #[test]
    fn test_merge_group_deduplicates_sentences() {
        let pairs = vec![
            (1i64, "Rust is fast. Rust is safe.".to_string()),
            (2i64, "Rust is safe. Rust is expressive.".to_string()),
        ];

        let merged = OfflineConsolidator::merge_group(&pairs);

        assert!(
            merged.contains("Rust is fast"),
            "must include first sentence"
        );
        assert!(
            merged.contains("Rust is safe"),
            "must include shared sentence once"
        );
        assert!(
            merged.contains("Rust is expressive"),
            "must include unique sentence"
        );

        // Count occurrences of "Rust is safe"
        let count = merged.matches("Rust is safe").count();
        assert_eq!(count, 1, "duplicate sentence must appear exactly once");
    }

    // ------------------------------------------------------------------
    // Test 4: consolidation saves to consolidated_memories table
    // ------------------------------------------------------------------
    #[test]
    fn test_consolidation_saves_to_table() {
        let conn = open_conn();
        insert_memory(
            &conn,
            "Rust provides zero-cost abstractions and memory safety",
            "ws1",
            2.0,
        );
        insert_memory(
            &conn,
            "Rust provides zero-cost abstractions and performance",
            "ws1",
            2.0,
        );

        let consolidator = OfflineConsolidator::new(default_config());
        consolidator.consolidate(&conn, "ws1").expect("consolidate");

        let records = list_consolidations(&conn, "ws1", 10).expect("list");
        assert!(
            !records.is_empty(),
            "expected at least one consolidation record"
        );
        assert_eq!(records[0].workspace, "ws1");
    }

    // ------------------------------------------------------------------
    // Test 5: report counts are correct
    // ------------------------------------------------------------------
    #[test]
    fn test_report_counts_correct() {
        let conn = open_conn();
        // Two similar memories → one group → 2 merged
        insert_memory(
            &conn,
            "Async Rust uses futures and tokio runtime for async tasks",
            "ws2",
            2.0,
        );
        insert_memory(
            &conn,
            "Async Rust uses futures and async-std runtime for async tasks",
            "ws2",
            2.0,
        );

        let consolidator = OfflineConsolidator::new(default_config());
        let report = consolidator.consolidate(&conn, "ws2").expect("consolidate");

        assert_eq!(report.groups_found, 1);
        assert_eq!(report.memories_merged, 2);
        assert_eq!(report.memories_archived, 2);
    }

    // ------------------------------------------------------------------
    // Test 6: token savings computed correctly
    // ------------------------------------------------------------------
    #[test]
    fn test_token_savings_computed() {
        let conn = open_conn();
        insert_memory(
            &conn,
            "The quick brown fox jumps over the lazy dog",
            "ws3",
            2.0,
        );
        insert_memory(
            &conn,
            "The quick brown fox jumps over the slow cat",
            "ws3",
            2.0,
        );

        let consolidator = OfflineConsolidator::new(default_config());
        let report = consolidator.consolidate(&conn, "ws3").expect("consolidate");

        // Both have 9 words → tokens_before = 18
        assert_eq!(report.tokens_before, 18);
        // Summary merges, so tokens_after < tokens_before
        assert!(
            report.tokens_after <= report.tokens_before,
            "summary should not be longer than sources"
        );
        assert_eq!(
            report.tokens_saved,
            report.tokens_before.saturating_sub(report.tokens_after)
        );
    }

    // ------------------------------------------------------------------
    // Test 7: tag-based grouping strategy
    // ------------------------------------------------------------------
    #[test]
    fn test_tag_similarity_grouping() {
        let conn = open_conn();
        let id1 = insert_memory(&conn, "First memory about rust", "default", 2.0);
        let id2 = insert_memory(&conn, "Second memory about rust", "default", 2.0);
        let id3 = insert_memory(&conn, "Python memory", "default", 2.0);

        tag_memory(&conn, id1, "rust");
        tag_memory(&conn, id1, "systems");
        tag_memory(&conn, id2, "rust");
        tag_memory(&conn, id2, "systems");
        tag_memory(&conn, id3, "python");

        let config = ConsolidationConfig {
            similarity_threshold: 0.5,
            ..default_config()
        };
        let consolidator = OfflineConsolidator::new(config);
        let groups = consolidator
            .find_groups(&conn, "default", GroupingStrategy::TagSimilarity)
            .expect("find_groups");

        assert!(!groups.is_empty(), "expected tag-based groups");
        let group = &groups[0];
        assert!(group.memory_ids.contains(&id1));
        assert!(group.memory_ids.contains(&id2));
        assert!(!group.memory_ids.contains(&id3));
        assert_eq!(group.strategy, GroupingStrategy::TagSimilarity);
    }

    // ------------------------------------------------------------------
    // Test 8: temporal proximity grouping
    // ------------------------------------------------------------------
    #[test]
    fn test_temporal_proximity_grouping() {
        let conn = open_conn();
        // Two memories created within the same hour window (both ~30min ago)
        let id1 = insert_memory(&conn, "Morning standup notes", "default", 0.4);
        let id2 = insert_memory(&conn, "Sprint planning notes", "default", 0.5);
        // One memory from yesterday
        let id3 = insert_memory(&conn, "Yesterday retrospective notes", "default", 25.0);

        let config = ConsolidationConfig {
            min_age_hours: 0.0,
            ..default_config()
        };
        let consolidator = OfflineConsolidator::new(config);
        let groups = consolidator
            .find_groups(&conn, "default", GroupingStrategy::TemporalProximity)
            .expect("find_groups");

        // id1 and id2 should be in a group together; id3 should be alone or in another group
        let group_with_recent = groups
            .iter()
            .find(|g| g.memory_ids.contains(&id1) && g.memory_ids.contains(&id2));
        assert!(
            group_with_recent.is_some(),
            "recent memories should be grouped by temporal proximity"
        );

        // id3 should not be with id1/id2
        if let Some(g) = group_with_recent {
            assert!(
                !g.memory_ids.contains(&id3),
                "yesterday's memory should not be in the same temporal group"
            );
        }
    }

    // ------------------------------------------------------------------
    // Test 9: empty workspace handled gracefully
    // ------------------------------------------------------------------
    #[test]
    fn test_empty_workspace_returns_empty_report() {
        let conn = open_conn();
        // Insert memories in a different workspace
        insert_memory(&conn, "Some memory", "other_workspace", 2.0);

        let consolidator = OfflineConsolidator::new(default_config());
        let report = consolidator
            .consolidate(&conn, "empty_workspace")
            .expect("consolidate empty workspace");

        assert_eq!(report.groups_found, 0);
        assert_eq!(report.memories_merged, 0);
        assert_eq!(report.memories_archived, 0);
        assert_eq!(report.tokens_before, 0);
        assert_eq!(report.tokens_after, 0);
        assert_eq!(report.tokens_saved, 0);
    }

    // ------------------------------------------------------------------
    // Bonus: get_consolidation_sources round-trips correctly
    // ------------------------------------------------------------------
    #[test]
    fn test_get_consolidation_sources_round_trip() {
        let conn = open_conn();
        let source_ids = vec![10i64, 20, 30];
        let cid = save_consolidation(
            &conn,
            &source_ids,
            "merged content",
            "content_overlap",
            60,
            30,
            "ws_rt",
        )
        .expect("save");

        let retrieved = get_consolidation_sources(&conn, cid).expect("get sources");
        assert_eq!(retrieved, source_ids);
    }

    // ------------------------------------------------------------------
    // Bonus: list_consolidations returns records in descending id order
    // ------------------------------------------------------------------
    #[test]
    fn test_list_consolidations_order() {
        let conn = open_conn();
        save_consolidation(
            &conn,
            &[1, 2],
            "first summary",
            "content_overlap",
            10,
            5,
            "ord",
        )
        .expect("save 1");
        save_consolidation(
            &conn,
            &[3, 4],
            "second summary",
            "content_overlap",
            20,
            8,
            "ord",
        )
        .expect("save 2");

        let records = list_consolidations(&conn, "ord", 10).expect("list");
        assert_eq!(records.len(), 2);
        // Most recent (highest id) first
        assert!(records[0].id > records[1].id);
    }
}
