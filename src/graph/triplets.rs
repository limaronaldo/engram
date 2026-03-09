//! Semantic Triplet Matching — RML-1219
//!
//! SPARQL-like pattern matching over the `facts` table.
//!
//! ## Features
//!
//! - [`TripletPattern`] — match facts by subject/predicate/object, with `None` as wildcard
//! - [`TripletMatcher::match_pattern`] — SQL WHERE clause matching with LIKE (case-insensitive)
//! - [`TripletMatcher::infer_transitive`] — BFS transitive inference over subject→object chains
//! - [`TripletMatcher::query_knowledge`] — simple NL-to-triplet query via entity extraction
//! - [`TripletMatcher::knowledge_stats`] — aggregate statistics over the facts table
//!
//! ## Invariants
//!
//! - `None` fields in [`TripletPattern`] match any value (wildcards)
//! - Matching is always case-insensitive via `lower()` / `LIKE`
//! - Transitive inference returns an empty vec when no paths exist
//! - `knowledge_stats` never panics — empty tables return zero-counts

use std::collections::{HashMap, HashSet, VecDeque};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::intelligence::fact_extraction::Fact;

// =============================================================================
// Types
// =============================================================================

/// A pattern for matching facts. `None` fields act as wildcards.
///
/// ```text
/// TripletPattern { subject: Some("Alice"), predicate: None, object: None }
/// // matches: Alice works_at Google, Alice lives_in Paris, …
/// ```
#[derive(Debug, Clone, Default)]
pub struct TripletPattern {
    /// If `Some(s)`, only facts where subject matches `s` (case-insensitive LIKE) are returned.
    pub subject: Option<String>,
    /// If `Some(p)`, only facts where predicate matches `p` (case-insensitive LIKE) are returned.
    pub predicate: Option<String>,
    /// If `Some(o)`, only facts where object matches `o` (case-insensitive LIKE) are returned.
    pub object: Option<String>,
}

impl TripletPattern {
    /// Create a pattern that matches everything (all wildcards).
    pub fn any() -> Self {
        Self::default()
    }

    pub fn with_subject(mut self, subject: impl Into<String>) -> Self {
        self.subject = Some(subject.into());
        self
    }

    pub fn with_predicate(mut self, predicate: impl Into<String>) -> Self {
        self.predicate = Some(predicate.into());
        self
    }

    pub fn with_object(mut self, object: impl Into<String>) -> Self {
        self.object = Some(object.into());
        self
    }
}

/// A single step in a transitive inference chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceStep {
    /// Subject of this fact
    pub subject: String,
    /// Predicate of this fact
    pub predicate: String,
    /// Object of this fact
    pub object: String,
    /// Database id of the source fact
    pub source_fact_id: i64,
}

/// A multi-hop inference path with aggregate confidence.
///
/// Confidence is the product of individual fact confidences along the path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferencePath {
    /// Ordered steps from the starting subject to the final object
    pub steps: Vec<InferenceStep>,
    /// Product of confidence scores along the path (in `[0.0, 1.0]`)
    pub confidence: f64,
}

/// Aggregate statistics about the knowledge base stored in the facts table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeStats {
    /// Total number of rows in the facts table
    pub total_facts: i64,
    /// Number of distinct subjects
    pub unique_subjects: i64,
    /// Number of distinct predicates
    pub unique_predicates: i64,
    /// Number of distinct objects
    pub unique_objects: i64,
    /// Top predicates sorted by frequency descending (predicate, count)
    pub top_predicates: Vec<(String, i64)>,
    /// Top subjects sorted by frequency descending (subject, count)
    pub top_subjects: Vec<(String, i64)>,
}

// =============================================================================
// TripletMatcher
// =============================================================================

/// Performs pattern-based and inference queries over the `facts` table.
pub struct TripletMatcher;

impl TripletMatcher {
    /// Match facts against a [`TripletPattern`].
    ///
    /// `None` fields are wildcards — they match any value.
    /// Non-`None` fields are matched case-insensitively with SQL `LIKE`.
    /// The `%` wildcard is automatically appended if the caller did not include it,
    /// but exact matching is used unless the caller embeds `%` themselves.
    ///
    /// Results are ordered by `id ASC`.
    pub fn match_pattern(conn: &Connection, pattern: &TripletPattern) -> Result<Vec<Fact>> {
        // Build WHERE clauses dynamically
        let mut conditions: Vec<String> = Vec::new();
        let mut bind_values: Vec<String> = Vec::new();

        if let Some(ref s) = pattern.subject {
            conditions.push(format!(
                "lower(subject) LIKE lower(?{})",
                conditions.len() + 1
            ));
            bind_values.push(s.clone());
        }
        if let Some(ref p) = pattern.predicate {
            conditions.push(format!(
                "lower(predicate) LIKE lower(?{})",
                conditions.len() + 1
            ));
            bind_values.push(p.clone());
        }
        if let Some(ref o) = pattern.object {
            conditions.push(format!(
                "lower(object) LIKE lower(?{})",
                conditions.len() + 1
            ));
            bind_values.push(o.clone());
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let sql = format!(
            "SELECT id, subject, predicate, object, confidence, source_memory_id, created_at
             FROM facts
             {where_clause}
             ORDER BY id ASC"
        );

        let mut stmt = conn.prepare(&sql)?;

        // rusqlite requires binding positional params; we map our vec to a slice of &dyn ToSql
        let facts = match bind_values.len() {
            0 => stmt
                .query_map([], map_row)?
                .collect::<std::result::Result<Vec<Fact>, _>>()?,
            1 => stmt
                .query_map(params![bind_values[0]], map_row)?
                .collect::<std::result::Result<Vec<Fact>, _>>()?,
            2 => stmt
                .query_map(params![bind_values[0], bind_values[1]], map_row)?
                .collect::<std::result::Result<Vec<Fact>, _>>()?,
            3 => stmt
                .query_map(
                    params![bind_values[0], bind_values[1], bind_values[2]],
                    map_row,
                )?
                .collect::<std::result::Result<Vec<Fact>, _>>()?,
            _ => unreachable!("pattern has at most 3 fields"),
        };

        Ok(facts)
    }

    /// Perform BFS transitive inference.
    ///
    /// Starting from `subject`, follows edges where the predicate matches `predicate`
    /// (exact, case-insensitive) up to `max_hops` deep.
    ///
    /// Each returned [`InferencePath`] represents one reachable chain. Cycles are
    /// avoided — a node is never visited twice in the same path.
    ///
    /// Returns an empty `Vec` when no matching edges exist.
    pub fn infer_transitive(
        conn: &Connection,
        subject: &str,
        predicate: &str,
        max_hops: usize,
    ) -> Result<Vec<InferencePath>> {
        if max_hops == 0 {
            return Ok(Vec::new());
        }

        // Load all edges that match the predicate once, build an adjacency map.
        // adjacency: subject (lowercase) -> Vec<(fact_id, subject_original, object, confidence)>
        let mut adj: HashMap<String, Vec<(i64, String, String, f64)>> = HashMap::new();

        {
            let mut stmt = conn.prepare(
                "SELECT id, subject, object, confidence
                 FROM facts
                 WHERE lower(predicate) = lower(?1)
                 ORDER BY id ASC",
            )?;

            let rows = stmt.query_map(params![predicate], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })?;

            for row in rows {
                let (fact_id, subj, obj, conf) = row?;
                adj.entry(subj.to_lowercase())
                    .or_default()
                    .push((fact_id, subj, obj, conf));
            }
        }

        // BFS: each queue item is (current_node_lowercase, path_so_far, path_confidence)
        let mut results: Vec<InferencePath> = Vec::new();
        let mut queue: VecDeque<(String, Vec<InferenceStep>, f64)> = VecDeque::new();
        queue.push_back((subject.to_lowercase(), Vec::new(), 1.0));

        while let Some((current, path, path_conf)) = queue.pop_front() {
            if path.len() >= max_hops {
                // We've reached the hop limit — record path if non-empty
                if !path.is_empty() {
                    results.push(InferencePath {
                        steps: path,
                        confidence: path_conf,
                    });
                }
                continue;
            }

            let neighbors = match adj.get(&current) {
                Some(n) => n.clone(),
                None => {
                    // Dead end — if we traversed at least one hop, record it
                    if !path.is_empty() {
                        results.push(InferencePath {
                            steps: path,
                            confidence: path_conf,
                        });
                    }
                    continue;
                }
            };

            // Visited set for cycle detection within this path
            let visited_in_path: HashSet<String> =
                path.iter().map(|s| s.subject.to_lowercase()).collect();

            let mut branched = false;
            for (fact_id, subj_orig, obj, conf) in neighbors {
                let obj_lower = obj.to_lowercase();

                // Avoid cycles
                if visited_in_path.contains(&obj_lower) || obj_lower == current {
                    continue;
                }

                let step = InferenceStep {
                    subject: subj_orig,
                    predicate: predicate.to_string(),
                    object: obj.clone(),
                    source_fact_id: fact_id,
                };

                let mut next_path = path.clone();
                next_path.push(step);
                let next_conf = path_conf * conf;
                queue.push_back((obj_lower, next_path, next_conf));
                branched = true;
            }

            // If we could not branch further and we already have steps, record
            if !branched && !path.is_empty() {
                results.push(InferencePath {
                    steps: path,
                    confidence: path_conf,
                });
            }
        }

        Ok(results)
    }

    /// Simple natural-language-to-triplet query.
    ///
    /// Extracts capitalized words from `natural_language` as potential entity names,
    /// then returns facts where `subject` or `object` matches any of those entities
    /// (case-insensitive LIKE).
    ///
    /// Returns an empty `Vec` when no entities can be extracted or no facts match.
    pub fn query_knowledge(conn: &Connection, natural_language: &str) -> Result<Vec<Fact>> {
        // Extract potential entities: words that start with an uppercase letter
        let entities: Vec<String> = natural_language
            .split_whitespace()
            .filter(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
            .map(|w| {
                // Strip trailing punctuation
                w.trim_end_matches(|c: char| !c.is_alphanumeric())
                    .to_string()
            })
            .filter(|w| w.len() >= 2)
            .collect::<std::collections::HashSet<_>>() // deduplicate
            .into_iter()
            .collect();

        if entities.is_empty() {
            return Ok(Vec::new());
        }

        // Build: WHERE lower(subject) IN (?,?,?) OR lower(object) IN (?,?,?)
        // We do this by running one query per entity and collecting unique fact IDs,
        // then returning the merged list ordered by id.
        let mut seen_ids: HashSet<i64> = HashSet::new();
        let mut all_facts: Vec<Fact> = Vec::new();

        let mut stmt = conn.prepare(
            "SELECT id, subject, predicate, object, confidence, source_memory_id, created_at
             FROM facts
             WHERE lower(subject) LIKE lower(?1) OR lower(object) LIKE lower(?1)
             ORDER BY id ASC",
        )?;

        for entity in &entities {
            let rows = stmt
                .query_map(params![entity], map_row)?
                .collect::<std::result::Result<Vec<Fact>, _>>()?;

            for fact in rows {
                if seen_ids.insert(fact.id) {
                    all_facts.push(fact);
                }
            }
        }

        // Sort by id for deterministic ordering
        all_facts.sort_by_key(|f| f.id);
        Ok(all_facts)
    }

    /// Compute aggregate statistics about the `facts` table.
    ///
    /// Returns zeroes for all counts when the table is empty.
    pub fn knowledge_stats(conn: &Connection) -> Result<KnowledgeStats> {
        let total_facts: i64 =
            conn.query_row("SELECT COUNT(*) FROM facts", [], |row| row.get(0))?;

        let unique_subjects: i64 =
            conn.query_row("SELECT COUNT(DISTINCT subject) FROM facts", [], |row| {
                row.get(0)
            })?;

        let unique_predicates: i64 =
            conn.query_row("SELECT COUNT(DISTINCT predicate) FROM facts", [], |row| {
                row.get(0)
            })?;

        let unique_objects: i64 =
            conn.query_row("SELECT COUNT(DISTINCT object) FROM facts", [], |row| {
                row.get(0)
            })?;

        // Top predicates (up to 10)
        let mut pred_stmt = conn.prepare(
            "SELECT predicate, COUNT(*) as cnt
             FROM facts
             GROUP BY predicate
             ORDER BY cnt DESC
             LIMIT 10",
        )?;
        let top_predicates: Vec<(String, i64)> = pred_stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        // Top subjects (up to 10)
        let mut subj_stmt = conn.prepare(
            "SELECT subject, COUNT(*) as cnt
             FROM facts
             GROUP BY subject
             ORDER BY cnt DESC
             LIMIT 10",
        )?;
        let top_subjects: Vec<(String, i64)> = subj_stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(KnowledgeStats {
            total_facts,
            unique_subjects,
            unique_predicates,
            unique_objects,
            top_predicates,
            top_subjects,
        })
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Fact> {
    Ok(Fact {
        id: row.get(0)?,
        subject: row.get(1)?,
        predicate: row.get(2)?,
        object: row.get(3)?,
        confidence: row.get(4)?,
        source_memory_id: row.get(5)?,
        created_at: row.get(6)?,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// DDL for the facts table — mirrors the production migration.
    const CREATE_TABLE: &str = r#"
        CREATE TABLE IF NOT EXISTS facts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            subject TEXT NOT NULL,
            predicate TEXT NOT NULL,
            object TEXT NOT NULL,
            confidence REAL NOT NULL DEFAULT 0.8,
            source_memory_id INTEGER,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
        );
        CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject);
    "#;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(CREATE_TABLE).expect("create table");
        conn
    }

    fn insert(conn: &Connection, subject: &str, predicate: &str, object: &str, confidence: f64) {
        conn.execute(
            "INSERT INTO facts (subject, predicate, object, confidence, created_at)
             VALUES (?1, ?2, ?3, ?4, '2026-01-01T00:00:00Z')",
            params![subject, predicate, object, confidence],
        )
        .expect("insert fact");
    }

    fn seed_graph(conn: &Connection) {
        // Alice -works_at-> Google -located_in-> California
        // Bob   -works_at-> Google
        // Carol -lives_in-> London
        // Dave  -located_in-> Paris
        insert(conn, "Alice", "works_at", "Google", 0.9);
        insert(conn, "Bob", "works_at", "Google", 0.85);
        insert(conn, "Google", "located_in", "California", 0.95);
        insert(conn, "Carol", "lives_in", "London", 0.8);
        insert(conn, "Dave", "located_in", "Paris", 0.75);
    }

    // -------------------------------------------------------------------------
    // match_pattern tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_match_by_subject() {
        let conn = setup();
        seed_graph(&conn);

        let pattern = TripletPattern::any().with_subject("Alice");
        let facts = TripletMatcher::match_pattern(&conn, &pattern).expect("match");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].subject, "Alice");
        assert_eq!(facts[0].predicate, "works_at");
        assert_eq!(facts[0].object, "Google");
    }

    #[test]
    fn test_match_by_predicate() {
        let conn = setup();
        seed_graph(&conn);

        let pattern = TripletPattern::any().with_predicate("works_at");
        let facts = TripletMatcher::match_pattern(&conn, &pattern).expect("match");
        assert_eq!(facts.len(), 2);
        let subjects: Vec<&str> = facts.iter().map(|f| f.subject.as_str()).collect();
        assert!(subjects.contains(&"Alice"));
        assert!(subjects.contains(&"Bob"));
    }

    #[test]
    fn test_match_by_object() {
        let conn = setup();
        seed_graph(&conn);

        let pattern = TripletPattern::any().with_object("Google");
        let facts = TripletMatcher::match_pattern(&conn, &pattern).expect("match");
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn test_wildcard_match_returns_all() {
        let conn = setup();
        seed_graph(&conn);

        let pattern = TripletPattern::any();
        let facts = TripletMatcher::match_pattern(&conn, &pattern).expect("match");
        assert_eq!(facts.len(), 5);
    }

    #[test]
    fn test_match_case_insensitive() {
        let conn = setup();
        seed_graph(&conn);

        // "alice" should match "Alice"
        let pattern = TripletPattern::any().with_subject("alice");
        let facts = TripletMatcher::match_pattern(&conn, &pattern).expect("match");
        assert_eq!(facts.len(), 1);
    }

    #[test]
    fn test_no_matches_returns_empty() {
        let conn = setup();
        seed_graph(&conn);

        let pattern = TripletPattern::any().with_subject("Nonexistent");
        let facts = TripletMatcher::match_pattern(&conn, &pattern).expect("match");
        assert!(facts.is_empty());
    }

    #[test]
    fn test_match_subject_and_predicate() {
        let conn = setup();
        seed_graph(&conn);

        let pattern = TripletPattern::any()
            .with_subject("Google")
            .with_predicate("located_in");
        let facts = TripletMatcher::match_pattern(&conn, &pattern).expect("match");
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].object, "California");
    }

    // -------------------------------------------------------------------------
    // infer_transitive tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_transitive_inference_two_hops() {
        let conn = setup();
        // Chain: Alice -works_at-> Google -works_at-> Alphabet
        insert(&conn, "Alice", "works_at", "Google", 0.9);
        insert(&conn, "Google", "works_at", "Alphabet", 0.8);

        let paths = TripletMatcher::infer_transitive(&conn, "Alice", "works_at", 3).expect("infer");

        assert!(!paths.is_empty(), "expected at least one inference path");
        // Find the longest path (Alice -> Google -> Alphabet)
        let longest = paths.iter().max_by_key(|p| p.steps.len()).unwrap();
        assert_eq!(longest.steps.len(), 2);
        assert_eq!(longest.steps[0].subject, "Alice");
        assert_eq!(longest.steps[0].object, "Google");
        assert_eq!(longest.steps[1].subject, "Google");
        assert_eq!(longest.steps[1].object, "Alphabet");
    }

    #[test]
    fn test_transitive_inference_no_matching_predicate() {
        let conn = setup();
        seed_graph(&conn);

        // "Alice" has works_at, but we query "lives_in" — no path should be found
        let paths = TripletMatcher::infer_transitive(&conn, "Alice", "lives_in", 3).expect("infer");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_transitive_inference_max_hops_zero() {
        let conn = setup();
        seed_graph(&conn);

        let paths = TripletMatcher::infer_transitive(&conn, "Alice", "works_at", 0).expect("infer");
        assert!(paths.is_empty());
    }

    #[test]
    fn test_transitive_confidence_product() {
        let conn = setup();
        // Two-hop chain with known confidences: 0.9 * 0.8 = 0.72
        insert(&conn, "Alice", "rel", "B", 0.9);
        insert(&conn, "B", "rel", "C", 0.8);

        let paths = TripletMatcher::infer_transitive(&conn, "Alice", "rel", 5).expect("infer");
        // Find path Alice->B->C (length 2)
        let two_hop = paths.iter().find(|p| p.steps.len() == 2);
        assert!(two_hop.is_some(), "expected 2-hop path");
        let conf = two_hop.unwrap().confidence;
        assert!(
            (conf - 0.9 * 0.8).abs() < 1e-9,
            "expected confidence ~0.72, got {conf}"
        );
    }

    // -------------------------------------------------------------------------
    // query_knowledge tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_query_knowledge_by_entity() {
        let conn = setup();
        seed_graph(&conn);

        let facts = TripletMatcher::query_knowledge(&conn, "What does Alice do?").expect("query");
        // "Alice" is capitalized — should find the Alice fact
        assert!(
            facts.iter().any(|f| f.subject == "Alice"),
            "expected Alice fact, got: {:?}",
            facts
        );
    }

    #[test]
    fn test_query_knowledge_no_entities_returns_empty() {
        let conn = setup();
        seed_graph(&conn);

        // All lowercase — no capitalized entities extracted
        let facts =
            TripletMatcher::query_knowledge(&conn, "what does everyone do?").expect("query");
        assert!(facts.is_empty());
    }

    #[test]
    fn test_query_knowledge_multiple_entities() {
        let conn = setup();
        seed_graph(&conn);

        // Both "Alice" and "Carol" are capitalized
        let facts =
            TripletMatcher::query_knowledge(&conn, "Tell me about Alice and Carol").expect("query");
        let subjects: Vec<&str> = facts.iter().map(|f| f.subject.as_str()).collect();
        assert!(subjects.contains(&"Alice"), "expected Alice fact");
        assert!(subjects.contains(&"Carol"), "expected Carol fact");
    }

    // -------------------------------------------------------------------------
    // knowledge_stats tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_knowledge_stats_empty_table() {
        let conn = setup();
        let stats = TripletMatcher::knowledge_stats(&conn).expect("stats");
        assert_eq!(stats.total_facts, 0);
        assert_eq!(stats.unique_subjects, 0);
        assert_eq!(stats.unique_predicates, 0);
        assert_eq!(stats.unique_objects, 0);
        assert!(stats.top_predicates.is_empty());
        assert!(stats.top_subjects.is_empty());
    }

    #[test]
    fn test_knowledge_stats_with_data() {
        let conn = setup();
        seed_graph(&conn);

        let stats = TripletMatcher::knowledge_stats(&conn).expect("stats");
        assert_eq!(stats.total_facts, 5);
        // Subjects: Alice, Bob, Google, Carol, Dave
        assert_eq!(stats.unique_subjects, 5);
        // Predicates: works_at, located_in, lives_in
        assert_eq!(stats.unique_predicates, 3);
        // Objects: Google (×2), California, London, Paris
        assert_eq!(stats.unique_objects, 4);

        // Top predicate should be works_at (appears 2×)
        assert!(!stats.top_predicates.is_empty());
        assert_eq!(stats.top_predicates[0].0, "works_at");
        assert_eq!(stats.top_predicates[0].1, 2);
    }
}
