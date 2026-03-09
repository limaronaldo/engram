//! Automatic fact extraction from free text — RML-1232
//!
//! Extracts structured subject-predicate-object triples from text using
//! regex-based pattern matching. Suitable for processing conversation logs,
//! notes, and other free-form memory content.
//!
//! ## Invariants
//!
//! - Extraction never panics on any input
//! - Empty/whitespace input returns empty results
//! - Duplicate SPO triples are deduplicated (highest confidence wins)
//! - Subjects are title-cased, subjects and objects are trimmed
//! - Confidence scores are in range [0.0, 1.0]

use std::collections::HashMap;

use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

// =============================================================================
// Types
// =============================================================================

/// A stored fact with a database-assigned id
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Database-assigned id
    pub id: i64,
    /// The entity the fact is about (e.g., "Alice")
    pub subject: String,
    /// The relationship type (e.g., "works_at")
    pub predicate: String,
    /// The value or target (e.g., "Google")
    pub object: String,
    /// Confidence in the extraction (0.0 – 1.0)
    pub confidence: f32,
    /// The memory this fact was extracted from, if any
    pub source_memory_id: Option<i64>,
    /// RFC3339 UTC timestamp
    pub created_at: String,
}

/// A fact before storage — no id yet
#[derive(Debug, Clone)]
pub struct ExtractedFact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub confidence: f32,
}

// =============================================================================
// Trait
// =============================================================================

/// Pluggable fact-extraction strategy
pub trait FactExtractor: Send + Sync {
    fn extract_facts(&self, text: &str) -> Vec<ExtractedFact>;
}

// =============================================================================
// Regex patterns (compiled once)
// =============================================================================

/// "{subject} is a {object}"  — more specific, checked before plain "is"
static IS_A_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+is\s+an?\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)")
        .expect("valid regex")
});

/// "{subject} is {object}"
static IS_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+is\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)",
    )
    .expect("valid regex")
});

/// "{subject} works at {object}"
static WORKS_AT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+works?\s+at\s+([A-Za-z0-9][A-Za-z0-9\s\.\-]{0,60}?)\b(?:[,\.\!]|$)")
        .expect("valid regex")
});

/// "{subject} lives in {object}"
static LIVES_IN_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+lives?\s+in\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)")
        .expect("valid regex")
});

/// "{subject} likes {object}"
static LIKES_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+likes?\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)",
    )
    .expect("valid regex")
});

/// "{subject} was born in {object}"
static BORN_IN_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+was\s+born\s+in\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)")
        .expect("valid regex")
});

/// "{subject} manages {object}"
static MANAGES_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+manages?\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)")
        .expect("valid regex")
});

/// "{subject} reports to {object}"
static REPORTS_TO_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+reports?\s+to\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)")
        .expect("valid regex")
});

/// "{subject} created {object}"
static CREATED_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b([A-Za-z][A-Za-z\s]{0,40}?)\s+created?\s+([A-Za-z][A-Za-z\s]{0,60}?)\b(?:[,\.\!]|$)")
        .expect("valid regex")
});

/// Structured "Key: value" patterns
static STRUCTURED_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?m)^(?:Name|Role|Location|Title|Company|Organization|Department|Team)\s*:\s*(.+)$",
    )
    .expect("valid regex")
});

// =============================================================================
// Rule-based extractor
// =============================================================================

/// Extracts facts using curated regex patterns.
///
/// Pattern confidence: exact relationship pattern = 0.8, structured field = 0.9
pub struct RuleBasedExtractor;

impl RuleBasedExtractor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RuleBasedExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl FactExtractor for RuleBasedExtractor {
    fn extract_facts(&self, text: &str) -> Vec<ExtractedFact> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }

        let mut facts = Vec::new();

        // -- Structured "Key: value" lines (confidence 0.9) --
        for cap in STRUCTURED_PATTERN.captures_iter(text) {
            if let (Some(key_m), Some(val_m)) = (cap.get(0), cap.get(1)) {
                // Extract the key from the full match
                let full = key_m.as_str();
                let colon_pos = full.find(':').unwrap_or(full.len());
                let key = full[..colon_pos].trim().to_lowercase().replace(' ', "_");
                let value = val_m.as_str().trim().to_string();

                if !key.is_empty() && !value.is_empty() {
                    // Use the value from the previous line as subject when possible.
                    // For structured blocks we use a placeholder "entity" as subject.
                    facts.push(ExtractedFact {
                        subject: "entity".to_string(),
                        predicate: key,
                        object: value,
                        confidence: 0.9,
                    });
                }
            }
        }

        // -- "is a/an" (more specific — before plain "is") --
        apply_pattern(&IS_A_PATTERN, text, "is_a", 0.8, &mut facts);

        // -- "is" (plain) --
        apply_pattern(&IS_PATTERN, text, "is", 0.8, &mut facts);

        // -- "works at" --
        apply_pattern(&WORKS_AT_PATTERN, text, "works_at", 0.8, &mut facts);

        // -- "lives in" --
        apply_pattern(&LIVES_IN_PATTERN, text, "lives_in", 0.8, &mut facts);

        // -- "likes" --
        apply_pattern(&LIKES_PATTERN, text, "likes", 0.8, &mut facts);

        // -- "was born in" --
        apply_pattern(&BORN_IN_PATTERN, text, "born_in", 0.8, &mut facts);

        // -- "manages" --
        apply_pattern(&MANAGES_PATTERN, text, "manages", 0.8, &mut facts);

        // -- "reports to" --
        apply_pattern(&REPORTS_TO_PATTERN, text, "reports_to", 0.8, &mut facts);

        // -- "created" --
        apply_pattern(&CREATED_PATTERN, text, "created", 0.8, &mut facts);

        facts
    }
}

/// Apply a two-capture-group pattern and push valid facts into `out`.
fn apply_pattern(
    pattern: &Regex,
    text: &str,
    predicate: &str,
    confidence: f32,
    out: &mut Vec<ExtractedFact>,
) {
    for cap in pattern.captures_iter(text) {
        let subject_raw = match cap.get(1) {
            Some(m) => m.as_str().trim(),
            None => continue,
        };
        let object_raw = match cap.get(2) {
            Some(m) => m.as_str().trim(),
            None => continue,
        };

        if subject_raw.is_empty() || object_raw.is_empty() {
            continue;
        }

        // Reject extremely short or long values
        if subject_raw.len() < 2 || object_raw.len() < 2 {
            continue;
        }

        out.push(ExtractedFact {
            subject: title_case(subject_raw),
            predicate: predicate.to_string(),
            object: object_raw.to_string(),
            confidence,
        });
    }
}

/// Title-case a string (first letter of each word capitalised).
fn title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().to_string() + &chars.as_str().to_lowercase(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// =============================================================================
// ConversationProcessor
// =============================================================================

/// Processes text (or a list of conversation messages) and deduplicates facts.
pub struct ConversationProcessor {
    extractor: Box<dyn FactExtractor>,
}

impl ConversationProcessor {
    pub fn new(extractor: Box<dyn FactExtractor>) -> Self {
        Self { extractor }
    }

    /// Extract facts from a single text, deduplicating by (subject, predicate, object).
    /// When duplicates exist the one with the highest confidence is kept.
    pub fn process_text(&self, text: &str, source_memory_id: Option<i64>) -> Vec<ExtractedFact> {
        let raw = self.extractor.extract_facts(text);
        let _ = source_memory_id; // kept for API symmetry; used by callers
        dedup_facts(raw)
    }

    /// Extract and deduplicate facts from a slice of messages.
    pub fn process_conversation(
        &self,
        messages: &[&str],
        source_memory_id: Option<i64>,
    ) -> Vec<ExtractedFact> {
        let _ = source_memory_id;
        let raw: Vec<ExtractedFact> = messages
            .iter()
            .flat_map(|msg| self.extractor.extract_facts(msg))
            .collect();
        dedup_facts(raw)
    }
}

/// Deduplicate a vec of facts by (subject, predicate, object), keeping the highest confidence.
fn dedup_facts(facts: Vec<ExtractedFact>) -> Vec<ExtractedFact> {
    let mut map: HashMap<(String, String, String), ExtractedFact> = HashMap::new();
    for fact in facts {
        let key = (
            fact.subject.clone(),
            fact.predicate.clone(),
            fact.object.clone(),
        );
        map.entry(key)
            .and_modify(|existing| {
                if fact.confidence > existing.confidence {
                    existing.confidence = fact.confidence;
                }
            })
            .or_insert(fact);
    }
    map.into_values().collect()
}

// =============================================================================
// Storage
// =============================================================================

/// DDL for the facts table — call once during schema setup.
pub const CREATE_FACTS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS facts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        subject TEXT NOT NULL,
        predicate TEXT NOT NULL,
        object TEXT NOT NULL,
        confidence REAL NOT NULL DEFAULT 0.5,
        source_memory_id INTEGER,
        created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
        UNIQUE(subject, predicate, object)
    );
    CREATE INDEX IF NOT EXISTS idx_facts_subject ON facts(subject);
    CREATE INDEX IF NOT EXISTS idx_facts_source ON facts(source_memory_id);
"#;

/// Insert or ignore a fact (UNIQUE constraint on SPO).
///
/// Returns the stored `Fact` with the database-assigned id.
pub fn create_fact(
    conn: &Connection,
    fact: &ExtractedFact,
    source_id: Option<i64>,
) -> Result<Fact> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    conn.execute(
        "INSERT OR IGNORE INTO facts (subject, predicate, object, confidence, source_memory_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            fact.subject,
            fact.predicate,
            fact.object,
            fact.confidence,
            source_id,
            now,
        ],
    )?;

    let stored = conn.query_row(
        "SELECT id, subject, predicate, object, confidence, source_memory_id, created_at
         FROM facts
         WHERE subject = ?1 AND predicate = ?2 AND object = ?3",
        params![fact.subject, fact.predicate, fact.object],
        |row| {
            Ok(Fact {
                id: row.get(0)?,
                subject: row.get(1)?,
                predicate: row.get(2)?,
                object: row.get(3)?,
                confidence: row.get(4)?,
                source_memory_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        },
    )?;

    Ok(stored)
}

/// List facts, optionally filtered by source memory id.
///
/// `limit = 0` is treated as unlimited (up to i64::MAX rows).
pub fn list_facts(
    conn: &Connection,
    source_memory_id: Option<i64>,
    limit: usize,
) -> Result<Vec<Fact>> {
    let effective_limit = if limit == 0 { i64::MAX } else { limit as i64 };

    let mut stmt = match source_memory_id {
        Some(sid) => {
            let mut s = conn.prepare(
                "SELECT id, subject, predicate, object, confidence, source_memory_id, created_at
                 FROM facts
                 WHERE source_memory_id = ?1
                 ORDER BY id ASC
                 LIMIT ?2",
            )?;
            let rows = s.query_map(params![sid, effective_limit], map_row)?;
            return rows
                .collect::<std::result::Result<Vec<Fact>, _>>()
                .map_err(Into::into);
        }
        None => conn.prepare(
            "SELECT id, subject, predicate, object, confidence, source_memory_id, created_at
             FROM facts
             ORDER BY id ASC
             LIMIT ?1",
        )?,
    };

    let rows = stmt.query_map(params![effective_limit], map_row)?;
    rows.collect::<std::result::Result<Vec<Fact>, _>>()
        .map_err(Into::into)
}

/// Return all facts whose subject matches (case-insensitive).
pub fn get_fact_graph(conn: &Connection, subject: &str) -> Result<Vec<Fact>> {
    let mut stmt = conn.prepare(
        "SELECT id, subject, predicate, object, confidence, source_memory_id, created_at
         FROM facts
         WHERE lower(subject) = lower(?1)
         ORDER BY id ASC",
    )?;
    let rows = stmt.query_map(params![subject], map_row)?;
    rows.collect::<std::result::Result<Vec<Fact>, _>>()
        .map_err(Into::into)
}

/// Delete all facts that were extracted from a given memory id.
///
/// Returns the number of rows deleted.
pub fn delete_facts_for_memory(conn: &Connection, memory_id: i64) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM facts WHERE source_memory_id = ?1",
        params![memory_id],
    )?;
    Ok(deleted)
}

/// Map a rusqlite row to a `Fact`
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

    fn make_extractor() -> RuleBasedExtractor {
        RuleBasedExtractor::new()
    }

    fn make_processor() -> ConversationProcessor {
        ConversationProcessor::new(Box::new(RuleBasedExtractor::new()))
    }

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(CREATE_FACTS_TABLE)
            .expect("create table");
        conn
    }

    // -------------------------------------------------------------------------
    // Extraction tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_extract_is_pattern() {
        let ex = make_extractor();
        let facts = ex.extract_facts("Alice is a developer");
        // Should match is_a
        assert!(!facts.is_empty(), "expected at least one fact");
        let fact = facts
            .iter()
            .find(|f| f.predicate == "is_a" || f.predicate == "is");
        assert!(fact.is_some(), "expected 'is_a' or 'is' predicate");
        let fact = fact.unwrap();
        assert!(
            fact.subject.to_lowercase().contains("alice"),
            "subject should be Alice, got {}",
            fact.subject
        );
    }

    #[test]
    fn test_extract_works_at() {
        let ex = make_extractor();
        let facts = ex.extract_facts("Bob works at Google");
        let fact = facts.iter().find(|f| f.predicate == "works_at");
        assert!(fact.is_some(), "expected works_at fact, got: {:?}", facts);
        let fact = fact.unwrap();
        assert!(fact.subject.to_lowercase().contains("bob"));
        assert!(fact.object.to_lowercase().contains("google"));
    }

    #[test]
    fn test_extract_lives_in() {
        let ex = make_extractor();
        let facts = ex.extract_facts("Carol lives in Tokyo");
        let fact = facts.iter().find(|f| f.predicate == "lives_in");
        assert!(fact.is_some(), "expected lives_in fact, got: {:?}", facts);
        let fact = fact.unwrap();
        assert!(fact.subject.to_lowercase().contains("carol"));
        assert!(fact.object.to_lowercase().contains("tokyo"));
    }

    #[test]
    fn test_extract_structured_field() {
        let ex = make_extractor();
        let text = "Name: David\nRole: Manager";
        let facts = ex.extract_facts(text);
        // Should extract at least name and role
        let has_name = facts
            .iter()
            .any(|f| f.predicate == "name" && f.object.contains("David"));
        let has_role = facts
            .iter()
            .any(|f| f.predicate == "role" && f.object.contains("Manager"));
        assert!(has_name, "expected name fact, got: {:?}", facts);
        assert!(has_role, "expected role fact, got: {:?}", facts);
    }

    #[test]
    fn test_extract_multiple_facts() {
        let ex = make_extractor();
        let text = "Emma works at Acme. She lives in Paris. Emma likes music.";
        let facts = ex.extract_facts(text);
        // At least 3 facts should be extracted
        assert!(
            facts.len() >= 3,
            "expected at least 3 facts, got {}: {:?}",
            facts.len(),
            facts
        );
    }

    #[test]
    fn test_dedup_same_fact() {
        // Two facts with same SPO but different confidence — keep highest
        let facts = vec![
            ExtractedFact {
                subject: "Alice".to_string(),
                predicate: "works_at".to_string(),
                object: "Acme".to_string(),
                confidence: 0.7,
            },
            ExtractedFact {
                subject: "Alice".to_string(),
                predicate: "works_at".to_string(),
                object: "Acme".to_string(),
                confidence: 0.9,
            },
        ];
        let deduped = dedup_facts(facts);
        assert_eq!(deduped.len(), 1);
        assert!(
            (deduped[0].confidence - 0.9).abs() < f32::EPSILON,
            "expected confidence 0.9, got {}",
            deduped[0].confidence
        );
    }

    #[test]
    fn test_empty_text() {
        let ex = make_extractor();
        assert!(ex.extract_facts("").is_empty());
        assert!(ex.extract_facts("   ").is_empty());
        assert!(ex.extract_facts("\n\t\n").is_empty());
    }

    #[test]
    fn test_conversation_processing() {
        let proc = make_processor();
        let messages = &[
            "Alice works at Google.",
            "Bob lives in London.",
            "Alice works at Google.", // duplicate
        ];
        let facts = proc.process_conversation(messages, None);
        // "Alice works at Google" should appear only once after dedup
        let alice_google: Vec<_> = facts
            .iter()
            .filter(|f| {
                f.predicate == "works_at"
                    && f.subject.to_lowercase().contains("alice")
                    && f.object.to_lowercase().contains("google")
            })
            .collect();
        assert_eq!(alice_google.len(), 1, "duplicate should be deduped");

        // Bob lives in London should also be present
        let bob_london = facts.iter().any(|f| {
            f.predicate == "lives_in"
                && f.subject.to_lowercase().contains("bob")
                && f.object.to_lowercase().contains("london")
        });
        assert!(bob_london, "expected Bob lives_in London fact");
    }

    // -------------------------------------------------------------------------
    // Storage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_storage_create_and_list() {
        let conn = in_memory_conn();

        let fact = ExtractedFact {
            subject: "Frank".to_string(),
            predicate: "works_at".to_string(),
            object: "Mozilla".to_string(),
            confidence: 0.85,
        };

        let stored = create_fact(&conn, &fact, Some(42)).expect("create fact");
        assert!(stored.id > 0);
        assert_eq!(stored.subject, "Frank");
        assert_eq!(stored.predicate, "works_at");
        assert_eq!(stored.object, "Mozilla");
        assert_eq!(stored.source_memory_id, Some(42));

        let all = list_facts(&conn, None, 100).expect("list facts");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, stored.id);
    }

    #[test]
    fn test_storage_fact_graph() {
        let conn = in_memory_conn();

        let facts_in = vec![
            ExtractedFact {
                subject: "Grace".to_string(),
                predicate: "works_at".to_string(),
                object: "Stripe".to_string(),
                confidence: 0.8,
            },
            ExtractedFact {
                subject: "Grace".to_string(),
                predicate: "lives_in".to_string(),
                object: "Dublin".to_string(),
                confidence: 0.8,
            },
            ExtractedFact {
                subject: "Henry".to_string(),
                predicate: "works_at".to_string(),
                object: "Stripe".to_string(),
                confidence: 0.8,
            },
        ];

        for f in &facts_in {
            create_fact(&conn, f, None).expect("create");
        }

        let graph = get_fact_graph(&conn, "Grace").expect("get graph");
        assert_eq!(graph.len(), 2);
        assert!(graph.iter().all(|f| f.subject == "Grace"));

        // Case-insensitive lookup
        let graph2 = get_fact_graph(&conn, "grace").expect("case insensitive");
        assert_eq!(graph2.len(), 2);
    }

    #[test]
    fn test_storage_delete_for_memory() {
        let conn = in_memory_conn();

        let f1 = ExtractedFact {
            subject: "Iris".to_string(),
            predicate: "works_at".to_string(),
            object: "Corp".to_string(),
            confidence: 0.8,
        };
        let f2 = ExtractedFact {
            subject: "Jack".to_string(),
            predicate: "lives_in".to_string(),
            object: "Berlin".to_string(),
            confidence: 0.8,
        };

        create_fact(&conn, &f1, Some(10)).expect("create f1");
        create_fact(&conn, &f2, Some(20)).expect("create f2");

        let deleted = delete_facts_for_memory(&conn, 10).expect("delete");
        assert_eq!(deleted, 1);

        let remaining = list_facts(&conn, None, 100).expect("list");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].subject, "Jack");
    }

    #[test]
    fn test_storage_list_filter_by_source() {
        let conn = in_memory_conn();

        for i in 0..3_i64 {
            let f = ExtractedFact {
                subject: format!("Person{}", i),
                predicate: "works_at".to_string(),
                object: "Acme".to_string(),
                confidence: 0.8,
            };
            create_fact(&conn, &f, Some(i + 1)).expect("create");
        }

        let filtered = list_facts(&conn, Some(2), 100).expect("list filtered");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].subject, "Person1");
    }

    #[test]
    fn test_title_case() {
        assert_eq!(title_case("alice"), "Alice");
        assert_eq!(title_case("alice smith"), "Alice Smith");
        assert_eq!(title_case("ALICE"), "Alice");
        assert_eq!(title_case(""), "");
    }

    #[test]
    fn test_confidence_range() {
        let ex = make_extractor();
        let facts = ex.extract_facts("Sam works at Acme. Name: Sam\nRole: Engineer.");
        for f in &facts {
            assert!(
                f.confidence >= 0.0 && f.confidence <= 1.0,
                "confidence out of range: {}",
                f.confidence
            );
        }
    }
}
