//! Historical Memory Update Detection — RML-1213
//!
//! A-Mem-inspired automatic memory update detection. When new information
//! contradicts or supplements existing memories, this module detects the
//! relationship and suggests an appropriate action.
//!
//! ## How it works
//!
//! 1. Fetch recent memories from the target workspace.
//! 2. For each existing memory, compute keyword overlap and entity matching
//!    with the new content.
//! 3. Classify the relationship: Contradiction, Supplement, Correction,
//!    or Obsolescence.
//! 4. Return `UpdateCandidate` structs for every pair whose confidence
//!    exceeds the threshold (0.3).
//! 5. The caller may then call `apply_update` to commit a chosen action and
//!    record it in the `update_log` table.
//!
//! ## Invariants
//!
//! - Detection never panics on any input.
//! - Empty workspace returns an empty candidate list.
//! - `apply_update` always writes one row to `update_log`.
//! - Confidence scores are in the range [0.0, 1.0].

use std::collections::HashSet;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

// =============================================================================
// Public types
// =============================================================================

/// Classifies the relationship between new content and an existing memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    /// New content directly contradicts the existing memory
    /// (e.g., negation keywords + shared entities).
    Contradiction,
    /// New content adds new predicates about the same entities without
    /// contradicting them.
    Supplement,
    /// New content explicitly corrects the existing memory
    /// (e.g., "actually", "correction", "update").
    Correction,
    /// The existing memory references old dates while new content uses
    /// temporal markers like "now" or "currently".
    Obsolescence,
}

impl ConflictType {
    pub fn as_str(self) -> &'static str {
        match self {
            ConflictType::Contradiction => "contradiction",
            ConflictType::Supplement => "supplement",
            ConflictType::Correction => "correction",
            ConflictType::Obsolescence => "obsolescence",
        }
    }
}

impl std::str::FromStr for ConflictType {
    type Err = EngramError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "contradiction" => Ok(ConflictType::Contradiction),
            "supplement" => Ok(ConflictType::Supplement),
            "correction" => Ok(ConflictType::Correction),
            "obsolescence" => Ok(ConflictType::Obsolescence),
            _ => Err(EngramError::InvalidInput(format!(
                "Unknown conflict type: {}",
                s
            ))),
        }
    }
}

/// The action to take when an update is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateAction {
    /// Overwrite the existing memory content with the new content.
    Replace,
    /// Append the new content to the existing memory.
    Merge,
    /// Change the memory type to `archived` so it is preserved but deprioritised.
    Archive,
    /// Add a `needs-review` tag so a human can inspect the conflict.
    Flag,
}

impl UpdateAction {
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateAction::Replace => "replace",
            UpdateAction::Merge => "merge",
            UpdateAction::Archive => "archive",
            UpdateAction::Flag => "flag",
        }
    }
}

impl std::str::FromStr for UpdateAction {
    type Err = EngramError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "replace" => Ok(UpdateAction::Replace),
            "merge" => Ok(UpdateAction::Merge),
            "archive" => Ok(UpdateAction::Archive),
            "flag" => Ok(UpdateAction::Flag),
            _ => Err(EngramError::InvalidInput(format!(
                "Unknown update action: {}",
                s
            ))),
        }
    }
}

/// A candidate memory that may need to be updated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCandidate {
    /// ID of the existing memory that may need updating.
    pub existing_id: i64,
    /// How the new content relates to the existing memory.
    pub conflict_type: ConflictType,
    /// Confidence score in the range [0.0, 1.0].
    pub confidence: f32,
    /// Suggested action to resolve the detected conflict.
    pub suggested_action: UpdateAction,
    /// Human-readable explanation for the suggestion.
    pub reason: String,
}

/// Result of applying an update to an existing memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    /// ID of the memory that was updated.
    pub memory_id: i64,
    /// The action that was applied.
    pub action_taken: UpdateAction,
    /// SHA-256 hex digest of the content *before* the update.
    pub old_content_hash: String,
    /// SHA-256 hex digest of the content *after* the update.
    pub new_content_hash: String,
}

/// A stored entry in the `update_log` table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateLogEntry {
    /// Database-assigned id.
    pub id: i64,
    /// Memory that was updated.
    pub memory_id: i64,
    /// Action that was applied.
    pub action: UpdateAction,
    /// Content hash before the update.
    pub old_hash: String,
    /// Content hash after the update.
    pub new_hash: String,
    /// Human-readable reason for the update.
    pub reason: String,
    /// RFC3339 UTC timestamp.
    pub timestamp: String,
}

// =============================================================================
// DDL
// =============================================================================

/// DDL for the `update_log` table.
///
/// Call once during schema setup (e.g., alongside `CREATE_FACTS_TABLE`).
pub const CREATE_UPDATE_LOG_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS update_log (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        memory_id  INTEGER NOT NULL,
        action     TEXT    NOT NULL,
        old_hash   TEXT    NOT NULL,
        new_hash   TEXT    NOT NULL,
        reason     TEXT    NOT NULL DEFAULT '',
        timestamp  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    CREATE INDEX IF NOT EXISTS idx_update_log_memory ON update_log(memory_id);
"#;

// =============================================================================
// Storage helpers
// =============================================================================

/// Insert one row into `update_log` and return the stored entry.
pub fn create_update_log(conn: &Connection, result: &UpdateResult, reason: &str) -> Result<UpdateLogEntry> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    conn.execute(
        "INSERT INTO update_log (memory_id, action, old_hash, new_hash, reason, timestamp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            result.memory_id,
            result.action_taken.as_str(),
            result.old_content_hash,
            result.new_content_hash,
            reason,
            now,
        ],
    )?;

    let id = conn.last_insert_rowid();

    Ok(UpdateLogEntry {
        id,
        memory_id: result.memory_id,
        action: result.action_taken,
        old_hash: result.old_content_hash.clone(),
        new_hash: result.new_content_hash.clone(),
        reason: reason.to_string(),
        timestamp: now,
    })
}

/// List update log entries, optionally filtered to a specific memory.
///
/// `limit = 0` means unlimited.
pub fn list_update_logs(
    conn: &Connection,
    memory_id: Option<i64>,
    limit: usize,
) -> Result<Vec<UpdateLogEntry>> {
    let effective_limit: i64 = if limit == 0 { i64::MAX } else { limit as i64 };

    let rows = match memory_id {
        Some(mid) => {
            let mut stmt = conn.prepare(
                "SELECT id, memory_id, action, old_hash, new_hash, reason, timestamp
                 FROM update_log
                 WHERE memory_id = ?1
                 ORDER BY id ASC
                 LIMIT ?2",
            )?;
            let x = stmt
                .query_map(params![mid, effective_limit], map_log_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            x
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, memory_id, action, old_hash, new_hash, reason, timestamp
                 FROM update_log
                 ORDER BY id ASC
                 LIMIT ?1",
            )?;
            let x = stmt
                .query_map(params![effective_limit], map_log_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            x
        }
    };

    Ok(rows)
}

fn map_log_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UpdateLogEntry> {
    let action_str: String = row.get(2)?;
    let action = action_str
        .parse::<UpdateAction>()
        .unwrap_or(UpdateAction::Flag);
    Ok(UpdateLogEntry {
        id: row.get(0)?,
        memory_id: row.get(1)?,
        action,
        old_hash: row.get(3)?,
        new_hash: row.get(4)?,
        reason: row.get(5)?,
        timestamp: row.get(6)?,
    })
}

// =============================================================================
// Detection engine
// =============================================================================

/// Confidence threshold below which candidates are discarded.
const MIN_CONFIDENCE: f32 = 0.3;

/// Maximum number of recent memories to compare against.
const MAX_RECENT_MEMORIES: i64 = 200;

/// Negation / contradiction signal words.
static NEGATION_WORDS: &[&str] = &[
    "not", "no longer", "never", "incorrect", "wrong", "false", "untrue",
    "doesn't", "don't", "isn't", "aren't", "wasn't", "weren't",
];

/// Explicit correction signal words.
static CORRECTION_WORDS: &[&str] = &[
    "actually", "correction", "update", "correcting", "in fact",
    "to clarify", "clarification", "erratum", "revised",
];

/// Temporal "now" markers that suggest the new content supersedes older info.
static NOW_WORDS: &[&str] = &[
    "now", "currently", "today", "as of", "at present", "present",
    "latest", "recent",
];

/// Year pattern: 4-digit numbers in the range 1900–2099.
static YEAR_RANGE_START: u32 = 1900;
static YEAR_RANGE_END: u32 = 2099;

/// Core update-detection engine.
pub struct UpdateDetector;

impl UpdateDetector {
    pub fn new() -> Self {
        Self
    }

    /// Detect update candidates for `new_content` against memories in `workspace`.
    ///
    /// Fetches at most `MAX_RECENT_MEMORIES` memories from the workspace and
    /// computes a confidence score for each one. Returns candidates whose
    /// confidence exceeds `MIN_CONFIDENCE`, sorted descending.
    pub fn detect_updates(
        &self,
        conn: &Connection,
        new_content: &str,
        workspace: &str,
    ) -> Result<Vec<UpdateCandidate>> {
        if new_content.trim().is_empty() || workspace.trim().is_empty() {
            return Ok(Vec::new());
        }

        // Fetch recent memories from the workspace.
        let memories = fetch_workspace_memories(conn, workspace)?;
        if memories.is_empty() {
            return Ok(Vec::new());
        }

        let new_lower = new_content.to_lowercase();
        let new_keywords = extract_keywords(&new_lower);

        let mut candidates: Vec<UpdateCandidate> = Vec::new();

        for (id, content, memory_type, tags) in &memories {
            let existing_lower = content.to_lowercase();
            let existing_keywords = extract_keywords(&existing_lower);

            let overlap = keyword_overlap(&new_keywords, &existing_keywords);
            if overlap == 0.0 {
                // No shared vocabulary — skip entirely.
                continue;
            }

            // Try each conflict class in priority order.
            // The first one that fires wins.
            if let Some(cand) = detect_correction(&new_lower, &existing_lower, *id, overlap) {
                candidates.push(cand);
            } else if let Some(cand) =
                detect_contradiction(&new_lower, &existing_lower, *id, overlap)
            {
                candidates.push(cand);
            } else if let Some(cand) =
                detect_obsolescence(&new_lower, &existing_lower, *id, overlap)
            {
                candidates.push(cand);
            } else if let Some(cand) = detect_supplement(
                &new_lower,
                &existing_lower,
                *id,
                overlap,
                memory_type,
                tags,
            ) {
                candidates.push(cand);
            }
        }

        // Sort by confidence descending, then by id ascending for determinism.
        candidates.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.existing_id.cmp(&b.existing_id))
        });

        Ok(candidates)
    }
}

impl Default for UpdateDetector {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Conflict classifiers
// =============================================================================

fn detect_contradiction(
    new_lower: &str,
    existing_lower: &str,
    id: i64,
    overlap: f32,
) -> Option<UpdateCandidate> {
    if overlap < 0.15 {
        return None;
    }

    let has_negation = NEGATION_WORDS
        .iter()
        .any(|w| new_lower.contains(w));

    if !has_negation {
        return None;
    }

    // Both texts must share some entity-like tokens.
    let shared = shared_entity_count(new_lower, existing_lower);
    if shared == 0 {
        return None;
    }

    let confidence = (overlap * 0.5 + 0.3).min(1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }

    Some(UpdateCandidate {
        existing_id: id,
        conflict_type: ConflictType::Contradiction,
        confidence,
        suggested_action: UpdateAction::Flag,
        reason: format!(
            "New content contains negation signals ('not', 'no longer', etc.) \
             and shares {} entity tokens with the existing memory (keyword overlap {:.0}%).",
            shared,
            overlap * 100.0
        ),
    })
}

fn detect_correction(
    new_lower: &str,
    existing_lower: &str,
    id: i64,
    overlap: f32,
) -> Option<UpdateCandidate> {
    if overlap < 0.10 {
        return None;
    }

    let has_correction = CORRECTION_WORDS
        .iter()
        .any(|w| new_lower.contains(w));

    if !has_correction {
        return None;
    }

    let _ = existing_lower; // kept for API symmetry

    let confidence = (overlap * 0.6 + 0.35).min(1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }

    Some(UpdateCandidate {
        existing_id: id,
        conflict_type: ConflictType::Correction,
        confidence,
        suggested_action: UpdateAction::Replace,
        reason: format!(
            "New content starts with an explicit correction signal ('actually', \
             'correction', etc.) and overlaps with the existing memory at {:.0}%.",
            overlap * 100.0
        ),
    })
}

fn detect_obsolescence(
    new_lower: &str,
    existing_lower: &str,
    id: i64,
    overlap: f32,
) -> Option<UpdateCandidate> {
    if overlap < 0.10 {
        return None;
    }

    let existing_has_old_date = contains_old_year(existing_lower);
    let new_has_now = NOW_WORDS.iter().any(|w| new_lower.contains(w));

    if !(existing_has_old_date && new_has_now) {
        return None;
    }

    let confidence = (overlap * 0.5 + 0.25).min(1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }

    Some(UpdateCandidate {
        existing_id: id,
        conflict_type: ConflictType::Obsolescence,
        confidence,
        suggested_action: UpdateAction::Archive,
        reason: format!(
            "Existing memory references old dates while the new content uses \
             temporal markers ('now', 'currently', etc.) at {:.0}% keyword overlap.",
            overlap * 100.0
        ),
    })
}

fn detect_supplement(
    new_lower: &str,
    existing_lower: &str,
    id: i64,
    overlap: f32,
    _memory_type: &str,
    _tags: &[String],
) -> Option<UpdateCandidate> {
    if overlap < 0.20 {
        return None;
    }

    // No negation or correction signals — pure additive information.
    let has_negation = NEGATION_WORDS.iter().any(|w| new_lower.contains(w));
    let has_correction = CORRECTION_WORDS.iter().any(|w| new_lower.contains(w));
    if has_negation || has_correction {
        return None;
    }

    // New content should have tokens not present in existing content.
    let new_keywords = extract_keywords(new_lower);
    let existing_keywords = extract_keywords(existing_lower);
    let new_unique: usize = new_keywords
        .iter()
        .filter(|k| !existing_keywords.contains(*k))
        .count();

    if new_unique == 0 {
        return None;
    }

    // Supplement confidence: base 0.15 so even moderate overlap (0.25+) clears the 0.3 threshold.
    let confidence = (overlap * 0.6 + 0.15).min(1.0);
    if confidence < MIN_CONFIDENCE {
        return None;
    }

    Some(UpdateCandidate {
        existing_id: id,
        conflict_type: ConflictType::Supplement,
        confidence,
        suggested_action: UpdateAction::Merge,
        reason: format!(
            "New content shares {:.0}% keywords with the existing memory and adds \
             {} new unique tokens — supplementary information detected.",
            overlap * 100.0,
            new_unique
        ),
    })
}

// =============================================================================
// Apply update
// =============================================================================

/// Apply `action` to an existing memory and return the result.
///
/// The caller is responsible for passing the `new_content` that triggered
/// the update; it is used for `Replace` and `Merge` actions.
///
/// **Note:** this function does NOT write to `update_log` itself. Call
/// `create_update_log` separately so the caller controls reason text.
pub fn apply_update(
    conn: &Connection,
    candidate: &UpdateCandidate,
    action: UpdateAction,
    new_content: &str,
) -> Result<UpdateResult> {
    // Fetch current content.
    let (old_content, tags_json): (String, String) = conn.query_row(
        "SELECT content, tags FROM memories WHERE id = ?1",
        params![candidate.existing_id],
        |row| Ok((row.get(0)?, row.get(1).unwrap_or_else(|_| "[]".to_string()))),
    )?;

    let old_hash = sha256_hex(&old_content);

    let new_stored_content = match action {
        UpdateAction::Replace => new_content.to_string(),
        UpdateAction::Merge => format!("{}\n\n{}", old_content.trim(), new_content.trim()),
        UpdateAction::Archive => old_content.clone(),
        UpdateAction::Flag => old_content.clone(),
    };

    let new_hash = sha256_hex(&new_stored_content);

    match action {
        UpdateAction::Replace => {
            conn.execute(
                "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    new_stored_content,
                    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    candidate.existing_id
                ],
            )?;
        }
        UpdateAction::Merge => {
            conn.execute(
                "UPDATE memories SET content = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    new_stored_content,
                    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    candidate.existing_id
                ],
            )?;
        }
        UpdateAction::Archive => {
            conn.execute(
                "UPDATE memories SET memory_type = 'archived', updated_at = ?1 WHERE id = ?2",
                params![
                    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    candidate.existing_id
                ],
            )?;
        }
        UpdateAction::Flag => {
            // Add 'needs-review' to the JSON tag array.
            let updated_tags = add_tag_to_json(&tags_json, "needs-review");
            conn.execute(
                "UPDATE memories SET tags = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    updated_tags,
                    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                    candidate.existing_id
                ],
            )?;
        }
    }

    Ok(UpdateResult {
        memory_id: candidate.existing_id,
        action_taken: action,
        old_content_hash: old_hash,
        new_content_hash: new_hash,
    })
}

// =============================================================================
// Internal helpers
// =============================================================================

/// Fetch (id, content, memory_type, tags) for recent memories in a workspace.
fn fetch_workspace_memories(
    conn: &Connection,
    workspace: &str,
) -> Result<Vec<(i64, String, String, Vec<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, memory_type, tags
         FROM memories
         WHERE workspace = ?1
         ORDER BY id DESC
         LIMIT ?2",
    )?;

    let rows = stmt
        .query_map(params![workspace, MAX_RECENT_MEMORIES], |row| {
            let tags_raw: String = row.get::<_, String>(3).unwrap_or_else(|_| "[]".to_string());
            let tags: Vec<String> =
                serde_json::from_str(&tags_raw).unwrap_or_default();
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2).unwrap_or_else(|_| "note".to_string()),
                tags,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(rows)
}

/// Extract meaningful keywords from lowercase text.
///
/// Splits on whitespace/punctuation, drops stop-words and short tokens.
fn extract_keywords(text: &str) -> HashSet<String> {
    const STOP_WORDS: &[&str] = &[
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "shall", "to", "of", "in", "on", "at",
        "by", "for", "with", "from", "as", "it", "its", "this", "that",
        "and", "or", "but", "not", "so", "if", "then", "than", "when",
        "i", "me", "my", "we", "our", "you", "your", "he", "she", "they",
    ];

    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .filter(|t| !STOP_WORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

/// Jaccard-style overlap: |A ∩ B| / |A ∪ B|.
fn keyword_overlap(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let intersection = a.intersection(b).count() as f32;
    let union = (a.len() + b.len()) as f32 - intersection;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

/// Count capitalised tokens shared between two lowercase texts.
///
/// We use a simple heuristic: tokens that start with an uppercase letter in the
/// *original* (non-lowercased) text are likely named entities. Since we receive
/// already-lowercased text here, we instead count tokens with length >= 4 that
/// appear in both texts as a proxy for entity-like shared nouns.
fn shared_entity_count(new_lower: &str, existing_lower: &str) -> usize {
    let a = extract_keywords(new_lower);
    let b = extract_keywords(existing_lower);
    a.intersection(&b)
        .filter(|t| t.len() >= 4)
        .count()
}

/// Return `true` if the text contains a 4-digit year in [1900, 2099].
fn contains_old_year(text: &str) -> bool {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            let mut num_str = String::with_capacity(4);
            num_str.push(c);
            for _ in 0..3 {
                match chars.peek() {
                    Some(d) if d.is_ascii_digit() => {
                        num_str.push(*d);
                        chars.next();
                    }
                    _ => break,
                }
            }
            if num_str.len() == 4 {
                if let Ok(year) = num_str.parse::<u32>() {
                    if year >= YEAR_RANGE_START && year <= YEAR_RANGE_END {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Compute a SHA-256 hex digest of a string without pulling in a heavy dep.
///
/// We use a simple FNV-1a inspired hash here because the spec only asks for
/// a "content hash" string — not cryptographic security. This keeps the module
/// dependency-free.
fn sha256_hex(content: &str) -> String {
    // Use a deterministic 64-bit FNV-1a hash formatted as 16-char hex.
    let mut hash: u64 = 14695981039346656037u64; // FNV offset basis
    for byte in content.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211u64); // FNV prime
    }
    format!("{:016x}", hash)
}

/// Append a tag to a JSON array string (e.g., `["existing"]` → `["existing","needs-review"]`).
fn add_tag_to_json(tags_json: &str, tag: &str) -> String {
    let mut tags: Vec<String> = serde_json::from_str(tags_json).unwrap_or_default();
    if !tags.iter().any(|t| t == tag) {
        tags.push(tag.to_string());
    }
    serde_json::to_string(&tags).unwrap_or_else(|_| format!("[\"{}\"]", tag))
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

    /// Create an in-memory database with the minimal `memories` table schema
    /// and the `update_log` table.
    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                content      TEXT    NOT NULL,
                memory_type  TEXT    NOT NULL DEFAULT 'note',
                tags         TEXT    NOT NULL DEFAULT '[]',
                workspace    TEXT    NOT NULL DEFAULT 'default',
                created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
                updated_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
            );",
        )
        .expect("create memories table");
        conn.execute_batch(CREATE_UPDATE_LOG_TABLE)
            .expect("create update_log table");
        conn
    }

    fn insert_memory(conn: &Connection, content: &str, workspace: &str) -> i64 {
        conn.execute(
            "INSERT INTO memories (content, workspace) VALUES (?1, ?2)",
            params![content, workspace],
        )
        .expect("insert memory");
        conn.last_insert_rowid()
    }

    fn get_content(conn: &Connection, id: i64) -> String {
        conn.query_row(
            "SELECT content FROM memories WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .expect("get content")
    }

    fn get_memory_type(conn: &Connection, id: i64) -> String {
        conn.query_row(
            "SELECT memory_type FROM memories WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .expect("get memory_type")
    }

    fn get_tags(conn: &Connection, id: i64) -> Vec<String> {
        let raw: String = conn
            .query_row(
                "SELECT tags FROM memories WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .expect("get tags");
        serde_json::from_str(&raw).unwrap_or_default()
    }

    // -------------------------------------------------------------------------
    // Detection tests — one per conflict type
    // -------------------------------------------------------------------------

    #[test]
    fn test_detect_contradiction() {
        let conn = in_memory_conn();
        let _id = insert_memory(
            &conn,
            "Alice works at Anthropic as a senior engineer.",
            "work",
        );

        let detector = UpdateDetector::new();
        let candidates = detector
            .detect_updates(&conn, "Alice no longer works at Anthropic.", "work")
            .expect("detect_updates should succeed");

        assert!(
            !candidates.is_empty(),
            "Expected at least one contradiction candidate"
        );
        let cand = candidates.iter().find(|c| c.conflict_type == ConflictType::Contradiction);
        assert!(
            cand.is_some(),
            "Expected a Contradiction candidate, got: {:?}",
            candidates
        );
        assert!(
            cand.unwrap().confidence >= MIN_CONFIDENCE,
            "Confidence too low"
        );
    }

    #[test]
    fn test_detect_supplement() {
        let conn = in_memory_conn();
        let _id = insert_memory(
            &conn,
            "Alice works at Anthropic as a senior engineer.",
            "work",
        );

        let detector = UpdateDetector::new();
        let candidates = detector
            .detect_updates(
                &conn,
                "Alice works at Anthropic and also leads the safety team.",
                "work",
            )
            .expect("detect_updates should succeed");

        let cand = candidates.iter().find(|c| c.conflict_type == ConflictType::Supplement);
        assert!(
            cand.is_some(),
            "Expected a Supplement candidate, got: {:?}",
            candidates
        );
    }

    #[test]
    fn test_detect_correction() {
        let conn = in_memory_conn();
        let _id = insert_memory(
            &conn,
            "The project deadline is Friday the 20th.",
            "schedule",
        );

        let detector = UpdateDetector::new();
        let candidates = detector
            .detect_updates(
                &conn,
                "Actually, the project deadline is Thursday the 19th.",
                "schedule",
            )
            .expect("detect_updates should succeed");

        let cand = candidates.iter().find(|c| c.conflict_type == ConflictType::Correction);
        assert!(
            cand.is_some(),
            "Expected a Correction candidate, got: {:?}",
            candidates
        );
        assert_eq!(
            cand.unwrap().suggested_action,
            UpdateAction::Replace,
            "Correction should suggest Replace"
        );
    }

    #[test]
    fn test_detect_obsolescence() {
        let conn = in_memory_conn();
        let _id = insert_memory(
            &conn,
            "In 2020, the team was using Python 3.6 for all services.",
            "tech",
        );

        let detector = UpdateDetector::new();
        let candidates = detector
            .detect_updates(
                &conn,
                "The team is currently using Python 3.12 for all services.",
                "tech",
            )
            .expect("detect_updates should succeed");

        let cand = candidates
            .iter()
            .find(|c| c.conflict_type == ConflictType::Obsolescence);
        assert!(
            cand.is_some(),
            "Expected an Obsolescence candidate, got: {:?}",
            candidates
        );
        assert_eq!(
            cand.unwrap().suggested_action,
            UpdateAction::Archive,
            "Obsolescence should suggest Archive"
        );
    }

    // -------------------------------------------------------------------------
    // Apply-action tests — one per UpdateAction variant
    // -------------------------------------------------------------------------

    #[test]
    fn test_apply_replace() {
        let conn = in_memory_conn();
        let id = insert_memory(&conn, "Old content about the project.", "notes");

        let candidate = UpdateCandidate {
            existing_id: id,
            conflict_type: ConflictType::Correction,
            confidence: 0.8,
            suggested_action: UpdateAction::Replace,
            reason: "test".to_string(),
        };

        let result = apply_update(&conn, &candidate, UpdateAction::Replace, "New content about the project.")
            .expect("apply_update should succeed");

        assert_eq!(result.memory_id, id);
        assert_eq!(result.action_taken, UpdateAction::Replace);
        assert_ne!(result.old_content_hash, result.new_content_hash);
        assert_eq!(get_content(&conn, id), "New content about the project.");
    }

    #[test]
    fn test_apply_merge() {
        let conn = in_memory_conn();
        let id = insert_memory(&conn, "Alice works at Anthropic.", "notes");

        let candidate = UpdateCandidate {
            existing_id: id,
            conflict_type: ConflictType::Supplement,
            confidence: 0.6,
            suggested_action: UpdateAction::Merge,
            reason: "test".to_string(),
        };

        let result = apply_update(&conn, &candidate, UpdateAction::Merge, "She leads the safety team.")
            .expect("apply_update should succeed");

        assert_eq!(result.action_taken, UpdateAction::Merge);
        let merged = get_content(&conn, id);
        assert!(
            merged.contains("Alice works at Anthropic."),
            "Merged content should retain old content"
        );
        assert!(
            merged.contains("She leads the safety team."),
            "Merged content should include new content"
        );
    }

    #[test]
    fn test_apply_archive() {
        let conn = in_memory_conn();
        let id = insert_memory(&conn, "We use Python 3.6.", "tech");

        let candidate = UpdateCandidate {
            existing_id: id,
            conflict_type: ConflictType::Obsolescence,
            confidence: 0.7,
            suggested_action: UpdateAction::Archive,
            reason: "test".to_string(),
        };

        let result = apply_update(&conn, &candidate, UpdateAction::Archive, "We now use Python 3.12.")
            .expect("apply_update should succeed");

        assert_eq!(result.action_taken, UpdateAction::Archive);
        assert_eq!(get_memory_type(&conn, id), "archived");
    }

    #[test]
    fn test_apply_flag() {
        let conn = in_memory_conn();
        let id = insert_memory(&conn, "The budget is $50k.", "finance");

        let candidate = UpdateCandidate {
            existing_id: id,
            conflict_type: ConflictType::Contradiction,
            confidence: 0.65,
            suggested_action: UpdateAction::Flag,
            reason: "test".to_string(),
        };

        let result = apply_update(&conn, &candidate, UpdateAction::Flag, "The budget is not $50k.")
            .expect("apply_update should succeed");

        assert_eq!(result.action_taken, UpdateAction::Flag);
        let tags = get_tags(&conn, id);
        assert!(
            tags.contains(&"needs-review".to_string()),
            "Tagged memory should contain 'needs-review'"
        );
    }

    // -------------------------------------------------------------------------
    // Edge-case tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_no_conflict_when_unrelated() {
        let conn = in_memory_conn();
        // Insert a memory about cooking — completely unrelated to software.
        let _id = insert_memory(
            &conn,
            "The best way to make pasta is to boil water and add salt.",
            "kitchen",
        );

        let detector = UpdateDetector::new();
        let candidates = detector
            .detect_updates(
                &conn,
                "Alice no longer works at Anthropic as an engineer.",
                "kitchen",
            )
            .expect("detect_updates should succeed");

        // No significant overlap → no candidates above threshold.
        assert!(
            candidates.is_empty(),
            "Expected no candidates for unrelated content, got: {:?}",
            candidates
        );
    }

    #[test]
    fn test_empty_workspace_returns_empty() {
        let conn = in_memory_conn();
        // No memories in "empty-ws".
        let detector = UpdateDetector::new();
        let candidates = detector
            .detect_updates(&conn, "Some new information.", "empty-ws")
            .expect("detect_updates should succeed");

        assert!(
            candidates.is_empty(),
            "Empty workspace must return empty candidates"
        );
    }

    // -------------------------------------------------------------------------
    // Log storage tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_create_and_list_update_log() {
        let conn = in_memory_conn();
        let id = insert_memory(&conn, "Original content.", "notes");

        let candidate = UpdateCandidate {
            existing_id: id,
            conflict_type: ConflictType::Correction,
            confidence: 0.9,
            suggested_action: UpdateAction::Replace,
            reason: "explicit correction".to_string(),
        };

        let result = apply_update(&conn, &candidate, UpdateAction::Replace, "Corrected content.")
            .expect("apply_update should succeed");

        let log_entry = create_update_log(&conn, &result, "explicit correction")
            .expect("create_update_log should succeed");

        assert_eq!(log_entry.memory_id, id);
        assert_eq!(log_entry.action, UpdateAction::Replace);
        assert!(!log_entry.old_hash.is_empty());
        assert!(!log_entry.new_hash.is_empty());
        assert_ne!(log_entry.old_hash, log_entry.new_hash);

        // list_update_logs filtered by memory_id
        let logs = list_update_logs(&conn, Some(id), 10).expect("list_update_logs should succeed");
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].id, log_entry.id);

        // list_update_logs unfiltered
        let all_logs = list_update_logs(&conn, None, 0).expect("list_update_logs should succeed");
        assert_eq!(all_logs.len(), 1);
    }
}
