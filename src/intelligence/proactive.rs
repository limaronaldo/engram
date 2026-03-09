//! Proactive Memory Acquisition — RML-1221
//!
//! Identifies knowledge gaps and suggests what the user should remember.
//!
//! ## Components
//! - [`GapDetector`] — analyses coverage, detects gaps, suggests acquisitions
//! - [`InterestTracker`] — records queries and surfaces frequent topics
//!
//! ## Invariants
//! - Confidence scores are always in the range [0.0, 1.0]
//! - Priority is 1 (highest) .. 3 (lowest)
//! - All timestamps are RFC3339 UTC
//! - No unwrap() in production paths

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

// =============================================================================
// Types
// =============================================================================

/// Summary of how well a workspace is covered by memories.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    /// Total number of memories in the workspace
    pub total_memories: i64,
    /// Number of memories per tag (topic)
    pub topic_distribution: Vec<(String, i64)>,
    /// Date ranges with no memories (gap > 7 days)
    pub temporal_gaps: Vec<TemporalGap>,
    /// Topics that are under-represented or low quality
    pub weak_areas: Vec<WeakArea>,
}

/// A period of time where no memories were created.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalGap {
    /// RFC3339 UTC timestamp — end of the previous memory
    pub from: String,
    /// RFC3339 UTC timestamp — start of the next memory
    pub to: String,
    /// Length of the gap in fractional days
    pub gap_days: f64,
}

/// A topic that needs more coverage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeakArea {
    /// The tag / topic label
    pub topic: String,
    /// How many memories carry this tag
    pub memory_count: i64,
    /// Average importance of memories on this topic
    pub avg_importance: f32,
    /// Human-readable suggestion for improvement
    pub suggestion: String,
}

/// A gap in the user's knowledge base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGap {
    /// Topic or category that is missing
    pub topic: String,
    /// How confident we are that this is a real gap (0.0 – 1.0)
    pub confidence: f32,
    /// Actionable suggestion for closing the gap
    pub suggestion: String,
    /// IDs of existing memories related to this gap
    pub related_memory_ids: Vec<i64>,
}

/// A concrete recommendation for a new memory to create.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcquisitionSuggestion {
    /// Hint for what the memory content should cover
    pub content_hint: String,
    /// Recommended memory type (e.g., "note", "decision", "todo")
    pub suggested_type: String,
    /// Priority: 1 = highest, 2 = medium, 3 = lowest
    pub priority: u8,
    /// Why this memory is worth creating
    pub reason: String,
}

// =============================================================================
// DDL
// =============================================================================

/// DDL for the query_log table — call once during schema setup.
pub const CREATE_QUERY_LOG_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS query_log (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        query     TEXT NOT NULL,
        workspace TEXT NOT NULL DEFAULT 'default',
        timestamp TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    CREATE INDEX IF NOT EXISTS idx_query_log_workspace ON query_log(workspace);
    CREATE INDEX IF NOT EXISTS idx_query_log_timestamp ON query_log(timestamp);
"#;

// =============================================================================
// GapDetector
// =============================================================================

/// Analyses a workspace and surfaces knowledge gaps.
pub struct GapDetector;

impl GapDetector {
    pub fn new() -> Self {
        Self
    }

    /// Analyse memory coverage for `workspace`.
    ///
    /// Returns a [`CoverageReport`] with tag distribution, temporal gaps,
    /// and weak areas.
    pub fn analyze_coverage(&self, conn: &Connection, workspace: &str) -> Result<CoverageReport> {
        // 1. Total memory count
        let total_memories: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE workspace = ?1",
            params![workspace],
            |row| row.get(0),
        )?;

        // 2. Topic distribution — count memories per tag
        let topic_distribution = self.count_memories_per_tag(conn, workspace)?;

        // 3. Temporal gaps — fetch sorted timestamps, look for gaps > 7 days
        let temporal_gaps = self.find_temporal_gaps(conn, workspace)?;

        // 4. Weak areas — tags with < 3 memories or avg importance < 0.3
        let weak_areas = self.find_weak_areas(conn, workspace)?;

        Ok(CoverageReport {
            total_memories,
            topic_distribution,
            temporal_gaps,
            weak_areas,
        })
    }

    /// Detect knowledge gaps for `workspace`.
    ///
    /// Three gap types are checked:
    /// - Sparse topics (< 3 memories per tag)
    /// - Temporal gaps (no memories for > 7 days)
    /// - Unresolved questions (memories containing `?`)
    pub fn detect_gaps(&self, conn: &Connection, workspace: &str) -> Result<Vec<KnowledgeGap>> {
        let mut gaps: Vec<KnowledgeGap> = Vec::new();

        // -- Sparse topics --
        let tag_counts = self.count_memories_per_tag(conn, workspace)?;
        for (tag, count) in &tag_counts {
            if *count < 3 {
                // Fetch IDs of memories with this tag so callers can inspect them
                let related_ids = self.memory_ids_for_tag(conn, workspace, tag)?;
                gaps.push(KnowledgeGap {
                    topic: tag.clone(),
                    confidence: 0.7,
                    suggestion: format!(
                        "Only {} memory/memories tagged '{}'. Consider adding more detail.",
                        count, tag
                    ),
                    related_memory_ids: related_ids,
                });
            }
        }

        // -- Temporal gaps --
        let temporal_gaps = self.find_temporal_gaps(conn, workspace)?;
        for gap in &temporal_gaps {
            gaps.push(KnowledgeGap {
                topic: format!("temporal gap ({:.1} days)", gap.gap_days),
                confidence: 0.5,
                suggestion: format!(
                    "No memories were created for {:.1} days between {} and {}. \
                     Consider adding a summary of what happened during this period.",
                    gap.gap_days, gap.from, gap.to
                ),
                related_memory_ids: vec![],
            });
        }

        // -- Unresolved questions --
        let question_ids = self.find_question_memory_ids(conn, workspace)?;
        if !question_ids.is_empty() {
            gaps.push(KnowledgeGap {
                topic: "unresolved questions".to_string(),
                confidence: 0.9,
                suggestion: format!(
                    "{} memory/memories contain unresolved questions. \
                     Recording answers will improve your knowledge base.",
                    question_ids.len()
                ),
                related_memory_ids: question_ids,
            });
        }

        Ok(gaps)
    }

    /// Suggest specific new memories that would close the most important gaps.
    ///
    /// Priority order: unresolved questions (1) > sparse topics (2) > temporal gaps (3).
    /// At most `limit` suggestions are returned (0 = unlimited).
    pub fn suggest_acquisitions(
        &self,
        conn: &Connection,
        workspace: &str,
        limit: usize,
    ) -> Result<Vec<AcquisitionSuggestion>> {
        let mut suggestions: Vec<AcquisitionSuggestion> = Vec::new();

        // Priority 1 — unresolved questions
        let question_ids = self.find_question_memory_ids(conn, workspace)?;
        if !question_ids.is_empty() {
            let count = question_ids.len();
            suggestions.push(AcquisitionSuggestion {
                content_hint: format!(
                    "Answer the {} outstanding question(s) stored in memories {:?}",
                    count, question_ids
                ),
                suggested_type: "note".to_string(),
                priority: 1,
                reason: format!(
                    "{} memories contain unanswered questions; capturing answers closes these gaps.",
                    count
                ),
            });
        }

        // Priority 2 — sparse topics
        let tag_counts = self.count_memories_per_tag(conn, workspace)?;
        for (tag, count) in &tag_counts {
            if *count < 3 {
                suggestions.push(AcquisitionSuggestion {
                    content_hint: format!(
                        "Add more information about '{}' (currently only {} memory/memories).",
                        tag, count
                    ),
                    suggested_type: "note".to_string(),
                    priority: 2,
                    reason: format!(
                        "The topic '{}' is under-represented with only {} entry/entries.",
                        tag, count
                    ),
                });
            }
        }

        // Priority 3 — temporal gaps
        let temporal_gaps = self.find_temporal_gaps(conn, workspace)?;
        for gap in &temporal_gaps {
            suggestions.push(AcquisitionSuggestion {
                content_hint: format!(
                    "Write a summary of events that occurred between {} and {} ({:.1} days).",
                    gap.from, gap.to, gap.gap_days
                ),
                suggested_type: "note".to_string(),
                priority: 3,
                reason: format!(
                    "There is a {:.1}-day gap in your memory timeline with no recorded events.",
                    gap.gap_days
                ),
            });
        }

        // Sort stable by priority (ascending = highest first)
        suggestions.sort_by_key(|s| s.priority);

        if limit > 0 {
            suggestions.truncate(limit);
        }

        Ok(suggestions)
    }

    // -------------------------------------------------------------------------
    // Private helpers
    // -------------------------------------------------------------------------

    /// Count memories per tag for the given workspace.
    fn count_memories_per_tag(
        &self,
        conn: &Connection,
        workspace: &str,
    ) -> Result<Vec<(String, i64)>> {
        // Tags are stored in a separate `tags` table linked to memories by memory_id.
        // We join on memories.workspace so we only count within the requested workspace.
        let mut stmt = conn.prepare(
            "SELECT t.tag, COUNT(DISTINCT t.memory_id) as cnt
             FROM tags t
             JOIN memories m ON m.id = t.memory_id
             WHERE m.workspace = ?1
             GROUP BY t.tag
             ORDER BY cnt DESC",
        )?;
        let rows = stmt.query_map(params![workspace], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let result: rusqlite::Result<Vec<(String, i64)>> = rows.collect();
        Ok(result?)
    }

    /// Find temporal gaps > 7 days in the workspace's memory timeline.
    fn find_temporal_gaps(&self, conn: &Connection, workspace: &str) -> Result<Vec<TemporalGap>> {
        let mut stmt = conn.prepare(
            "SELECT created_at FROM memories
             WHERE workspace = ?1
             ORDER BY created_at ASC",
        )?;
        let timestamps: Vec<String> = stmt
            .query_map(params![workspace], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;

        let mut gaps = Vec::new();
        for window in timestamps.windows(2) {
            let from_str = &window[0];
            let to_str = &window[1];

            // Parse as naive datetime (RFC3339) — fall back gracefully on error
            if let (Ok(from_dt), Ok(to_dt)) = (
                chrono::DateTime::parse_from_rfc3339(from_str),
                chrono::DateTime::parse_from_rfc3339(to_str),
            ) {
                let gap_seconds = (to_dt - from_dt).num_seconds();
                let gap_days = gap_seconds as f64 / 86_400.0;
                if gap_days > 7.0 {
                    gaps.push(TemporalGap {
                        from: from_str.clone(),
                        to: to_str.clone(),
                        gap_days,
                    });
                }
            }
        }
        Ok(gaps)
    }

    /// Find tags that are weak: fewer than 3 memories OR avg importance < 0.3.
    fn find_weak_areas(&self, conn: &Connection, workspace: &str) -> Result<Vec<WeakArea>> {
        let mut stmt = conn.prepare(
            "SELECT t.tag,
                    COUNT(DISTINCT t.memory_id)       AS cnt,
                    AVG(COALESCE(m.importance, 0.5))  AS avg_imp
             FROM tags t
             JOIN memories m ON m.id = t.memory_id
             WHERE m.workspace = ?1
             GROUP BY t.tag",
        )?;
        let rows = stmt.query_map(params![workspace], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;

        let mut weak: Vec<WeakArea> = Vec::new();
        for row in rows {
            let (tag, count, avg_imp) = row?;
            let avg_importance = avg_imp as f32;
            if count < 3 || avg_importance < 0.3 {
                let suggestion = if count < 3 {
                    format!(
                        "Only {} memory/memories about '{}'. Expand coverage.",
                        count, tag
                    )
                } else {
                    format!(
                        "Memories about '{}' have low average importance ({:.2}). \
                         Review and update their relevance.",
                        tag, avg_importance
                    )
                };
                weak.push(WeakArea {
                    topic: tag,
                    memory_count: count,
                    avg_importance,
                    suggestion,
                });
            }
        }
        Ok(weak)
    }

    /// Return memory IDs for a given tag within a workspace.
    fn memory_ids_for_tag(
        &self,
        conn: &Connection,
        workspace: &str,
        tag: &str,
    ) -> Result<Vec<i64>> {
        let mut stmt = conn.prepare(
            "SELECT t.memory_id FROM tags t
             JOIN memories m ON m.id = t.memory_id
             WHERE m.workspace = ?1 AND t.tag = ?2
             ORDER BY t.memory_id ASC",
        )?;
        let ids: Vec<i64> = stmt
            .query_map(params![workspace, tag], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }

    /// Return IDs of memories whose content contains `?`.
    fn find_question_memory_ids(&self, conn: &Connection, workspace: &str) -> Result<Vec<i64>> {
        let mut stmt = conn.prepare(
            "SELECT id FROM memories
             WHERE workspace = ?1 AND content LIKE '%?%'
             ORDER BY id ASC",
        )?;
        let ids: Vec<i64> = stmt
            .query_map(params![workspace], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        Ok(ids)
    }
}

impl Default for GapDetector {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// InterestTracker
// =============================================================================

/// Tracks user queries to surface topics the user is interested in.
pub struct InterestTracker;

impl InterestTracker {
    pub fn new() -> Self {
        Self
    }

    /// Record a search query for later analysis.
    ///
    /// The query_log table must already exist (see [`CREATE_QUERY_LOG_TABLE`]).
    pub fn record_query(&self, conn: &Connection, query: &str, workspace: &str) -> Result<()> {
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        conn.execute(
            "INSERT INTO query_log (query, workspace, timestamp) VALUES (?1, ?2, ?3)",
            params![query, workspace, now],
        )?;
        Ok(())
    }

    /// Return the most frequently queried keywords for `workspace`.
    ///
    /// Each query is split into lowercase words; counts are aggregated.
    /// Returns at most `limit` results ordered by frequency descending.
    /// `limit = 0` returns all results.
    pub fn get_frequent_topics(
        &self,
        conn: &Connection,
        workspace: &str,
        limit: usize,
    ) -> Result<Vec<(String, i64)>> {
        // Fetch all queries for the workspace
        let mut stmt = conn.prepare("SELECT query FROM query_log WHERE workspace = ?1")?;
        let queries: Vec<String> = stmt
            .query_map(params![workspace], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;

        // Count word frequencies (stop-word filtering)
        let stop_words: std::collections::HashSet<&str> = [
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "have", "has", "had", "do",
            "does", "did", "will", "would", "could", "should", "this", "that", "and", "but", "or",
            "if", "in", "on", "at", "by", "to", "of", "for", "with", "from", "as", "it", "its",
            "not", "no",
        ]
        .iter()
        .cloned()
        .collect();

        let mut counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();

        for query in &queries {
            for word in query.split_whitespace() {
                let w = word
                    .to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string();
                if w.len() > 2 && !stop_words.contains(w.as_str()) {
                    *counts.entry(w).or_insert(0) += 1;
                }
            }
        }

        let mut sorted: Vec<(String, i64)> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        if limit > 0 {
            sorted.truncate(limit);
        }
        Ok(sorted)
    }
}

impl Default for InterestTracker {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Create an in-memory SQLite database with minimal schema for testing.
    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                content     TEXT    NOT NULL,
                workspace   TEXT    NOT NULL DEFAULT 'default',
                importance  REAL    NOT NULL DEFAULT 0.5,
                created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE TABLE IF NOT EXISTS tags (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                memory_id   INTEGER NOT NULL,
                tag         TEXT    NOT NULL,
                FOREIGN KEY(memory_id) REFERENCES memories(id)
            );",
        )
        .unwrap();

        conn.execute_batch(CREATE_QUERY_LOG_TABLE).unwrap();

        conn
    }

    /// Insert a memory with optional tags and importance.
    fn insert_memory(
        conn: &Connection,
        workspace: &str,
        content: &str,
        importance: f32,
        created_at: &str,
        tags: &[&str],
    ) -> i64 {
        conn.execute(
            "INSERT INTO memories (content, workspace, importance, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![content, workspace, importance, created_at],
        )
        .unwrap();
        let id = conn.last_insert_rowid();
        for tag in tags {
            conn.execute(
                "INSERT INTO tags (memory_id, tag) VALUES (?1, ?2)",
                params![id, tag],
            )
            .unwrap();
        }
        id
    }

    // -------------------------------------------------------------------------
    // Test 1 — coverage report with varied data
    // -------------------------------------------------------------------------
    #[test]
    fn test_coverage_report_with_varied_data() {
        let conn = setup_conn();

        insert_memory(
            &conn,
            "ws",
            "Rust basics",
            0.8,
            "2024-01-01T00:00:00Z",
            &["rust", "programming"],
        );
        insert_memory(
            &conn,
            "ws",
            "Rust lifetimes",
            0.7,
            "2024-01-02T00:00:00Z",
            &["rust"],
        );
        insert_memory(
            &conn,
            "ws",
            "Rust traits",
            0.9,
            "2024-01-03T00:00:00Z",
            &["rust"],
        );
        insert_memory(
            &conn,
            "ws",
            "Python basics",
            0.5,
            "2024-01-04T00:00:00Z",
            &["python"],
        );

        let detector = GapDetector::new();
        let report = detector.analyze_coverage(&conn, "ws").unwrap();

        assert_eq!(report.total_memories, 4);

        // "rust" should be the most represented topic
        let rust_count = report
            .topic_distribution
            .iter()
            .find(|(tag, _)| tag == "rust")
            .map(|(_, c)| *c)
            .unwrap_or(0);
        assert_eq!(rust_count, 3);

        // "python" and "programming" each appear once → weak areas
        let python_weak = report.weak_areas.iter().any(|w| w.topic == "python");
        assert!(python_weak, "python should be a weak area (only 1 memory)");
    }

    // -------------------------------------------------------------------------
    // Test 2 — temporal gap detection
    // -------------------------------------------------------------------------
    #[test]
    fn test_temporal_gap_detection() {
        let conn = setup_conn();

        // Two memories ~20 days apart — should produce a gap
        insert_memory(&conn, "ws", "Note A", 0.5, "2024-03-01T00:00:00Z", &[]);
        insert_memory(&conn, "ws", "Note B", 0.5, "2024-03-21T00:00:00Z", &[]);

        let detector = GapDetector::new();
        let report = detector.analyze_coverage(&conn, "ws").unwrap();

        assert!(
            !report.temporal_gaps.is_empty(),
            "should detect a 20-day gap"
        );
        let gap = &report.temporal_gaps[0];
        assert!(gap.gap_days > 19.0 && gap.gap_days < 21.0);
    }

    // -------------------------------------------------------------------------
    // Test 3 — no temporal gap when memories are within 7 days
    // -------------------------------------------------------------------------
    #[test]
    fn test_no_temporal_gap_within_7_days() {
        let conn = setup_conn();

        insert_memory(&conn, "ws", "Note A", 0.5, "2024-03-01T00:00:00Z", &[]);
        insert_memory(&conn, "ws", "Note B", 0.5, "2024-03-05T00:00:00Z", &[]);

        let detector = GapDetector::new();
        let report = detector.analyze_coverage(&conn, "ws").unwrap();

        assert!(
            report.temporal_gaps.is_empty(),
            "4-day gap should not be reported"
        );
    }

    // -------------------------------------------------------------------------
    // Test 4 — weak area detection (low importance)
    // -------------------------------------------------------------------------
    #[test]
    fn test_weak_area_low_importance() {
        let conn = setup_conn();

        // Three memories tagged "low-imp" with very low importance
        for i in 0..3 {
            insert_memory(
                &conn,
                "ws",
                &format!("Low importance note {}", i),
                0.1,
                &format!("2024-05-0{}T00:00:00Z", i + 1),
                &["low-imp"],
            );
        }
        // One high importance memory tagged "high-imp"
        insert_memory(
            &conn,
            "ws",
            "Important note",
            0.9,
            "2024-05-10T00:00:00Z",
            &["high-imp"],
        );

        let detector = GapDetector::new();
        let report = detector.analyze_coverage(&conn, "ws").unwrap();

        let low_imp_weak = report
            .weak_areas
            .iter()
            .any(|w| w.topic == "low-imp" && w.avg_importance < 0.3);
        assert!(low_imp_weak, "low-imp should be flagged as weak area");

        let high_imp_weak = report.weak_areas.iter().any(|w| w.topic == "high-imp");
        // high-imp has only 1 memory → still weak by count; that's expected
        assert!(
            high_imp_weak,
            "high-imp has only 1 memory, still a weak area by count"
        );
    }

    // -------------------------------------------------------------------------
    // Test 5 — suggest acquisitions priority order
    // -------------------------------------------------------------------------
    #[test]
    fn test_suggest_acquisitions_priority_order() {
        let conn = setup_conn();

        // Unresolved question → priority 1
        insert_memory(
            &conn,
            "ws",
            "What is the best caching strategy?",
            0.5,
            "2024-06-01T00:00:00Z",
            &[],
        );

        // Sparse topic → priority 2
        insert_memory(
            &conn,
            "ws",
            "Note about caching",
            0.5,
            "2024-06-02T00:00:00Z",
            &["caching"],
        );

        // Temporal gap → priority 3
        insert_memory(
            &conn,
            "ws",
            "Note before gap",
            0.5,
            "2024-01-01T00:00:00Z",
            &[],
        );
        insert_memory(
            &conn,
            "ws",
            "Note after gap",
            0.5,
            "2024-03-01T00:00:00Z",
            &[],
        );

        let detector = GapDetector::new();
        let suggestions = detector.suggest_acquisitions(&conn, "ws", 10).unwrap();

        assert!(!suggestions.is_empty());

        // First suggestion must be priority 1 (unresolved questions)
        assert_eq!(
            suggestions[0].priority, 1,
            "first suggestion should be priority 1 (unresolved question)"
        );

        // Verify all priorities are in non-decreasing order
        for window in suggestions.windows(2) {
            assert!(
                window[0].priority <= window[1].priority,
                "suggestions should be sorted by priority ascending"
            );
        }
    }

    // -------------------------------------------------------------------------
    // Test 6 — interest tracking
    // -------------------------------------------------------------------------
    #[test]
    fn test_interest_tracking() {
        let conn = setup_conn();
        let tracker = InterestTracker::new();

        tracker
            .record_query(&conn, "rust async programming", "ws")
            .unwrap();
        tracker
            .record_query(&conn, "rust error handling", "ws")
            .unwrap();
        tracker.record_query(&conn, "rust lifetimes", "ws").unwrap();
        tracker
            .record_query(&conn, "python web frameworks", "ws")
            .unwrap();

        let topics = tracker.get_frequent_topics(&conn, "ws", 5).unwrap();

        assert!(!topics.is_empty());
        // "rust" appears 3 times, should be the top topic
        let rust_entry = topics.iter().find(|(word, _)| word == "rust");
        assert!(
            rust_entry.is_some(),
            "rust should appear in frequent topics"
        );
        assert_eq!(rust_entry.unwrap().1, 3, "rust should have count 3");
    }

    // -------------------------------------------------------------------------
    // Test 7 — empty workspace
    // -------------------------------------------------------------------------
    #[test]
    fn test_empty_workspace() {
        let conn = setup_conn();
        let detector = GapDetector::new();

        let report = detector.analyze_coverage(&conn, "empty-ws").unwrap();

        assert_eq!(report.total_memories, 0);
        assert!(report.topic_distribution.is_empty());
        assert!(report.temporal_gaps.is_empty());
        assert!(report.weak_areas.is_empty());

        let gaps = detector.detect_gaps(&conn, "empty-ws").unwrap();
        assert!(gaps.is_empty());

        let suggestions = detector
            .suggest_acquisitions(&conn, "empty-ws", 10)
            .unwrap();
        assert!(suggestions.is_empty());
    }

    // -------------------------------------------------------------------------
    // Test 8 — gap detection with unresolved questions
    // -------------------------------------------------------------------------
    #[test]
    fn test_gap_detection_with_questions() {
        let conn = setup_conn();

        let id1 = insert_memory(
            &conn,
            "ws",
            "How does tokio handle backpressure?",
            0.7,
            "2024-07-01T00:00:00Z",
            &[],
        );
        let id2 = insert_memory(
            &conn,
            "ws",
            "What is the difference between Arc and Rc?",
            0.6,
            "2024-07-02T00:00:00Z",
            &[],
        );

        let detector = GapDetector::new();
        let gaps = detector.detect_gaps(&conn, "ws").unwrap();

        let question_gap = gaps.iter().find(|g| g.topic == "unresolved questions");
        assert!(
            question_gap.is_some(),
            "should detect unresolved questions gap"
        );

        let qg = question_gap.unwrap();
        assert!(qg.related_memory_ids.contains(&id1));
        assert!(qg.related_memory_ids.contains(&id2));
        assert!(
            qg.confidence > 0.8,
            "confidence for question gaps should be high"
        );
    }

    // -------------------------------------------------------------------------
    // Test 9 — interest tracker respects workspace isolation
    // -------------------------------------------------------------------------
    #[test]
    fn test_interest_tracker_workspace_isolation() {
        let conn = setup_conn();
        let tracker = InterestTracker::new();

        tracker
            .record_query(&conn, "machine learning concepts", "ml-ws")
            .unwrap();
        tracker
            .record_query(&conn, "deep learning tutorial", "ml-ws")
            .unwrap();
        tracker
            .record_query(&conn, "rust ownership", "rust-ws")
            .unwrap();

        let ml_topics = tracker.get_frequent_topics(&conn, "ml-ws", 10).unwrap();
        let rust_topics = tracker.get_frequent_topics(&conn, "rust-ws", 10).unwrap();

        // ml-ws should not contain "rust"
        assert!(!ml_topics.iter().any(|(w, _)| w == "rust"));
        // rust-ws should not contain "learning"
        assert!(!rust_topics.iter().any(|(w, _)| w == "learning"));
    }

    // -------------------------------------------------------------------------
    // Test 10 — suggest acquisitions respects limit
    // -------------------------------------------------------------------------
    #[test]
    fn test_suggest_acquisitions_limit() {
        let conn = setup_conn();

        // Create multiple sparse topics to generate many suggestions
        for i in 0..5 {
            insert_memory(
                &conn,
                "ws",
                &format!("Note about topic {}", i),
                0.5,
                &format!("2024-08-0{}T00:00:00Z", i + 1),
                &[&format!("topic-{}", i)],
            );
        }

        let detector = GapDetector::new();
        let suggestions = detector.suggest_acquisitions(&conn, "ws", 3).unwrap();

        assert!(suggestions.len() <= 3, "should respect the limit");
    }
}
