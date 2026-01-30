//! Context Quality Module (Phase 9: ENG-48 to ENG-66)
//!
//! Provides:
//! - Near-duplicate detection (ENG-48)
//! - Semantic deduplication (ENG-49)
//! - Conflict detection (ENG-50)
//! - Contradiction resolution (ENG-51)
//! - Enhanced quality scoring (ENG-52)
//! - Source credibility (ENG-53)
//! - Quality improvement suggestions (ENG-57)

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::error::{EngramError, Result};
use crate::storage::queries::get_memory;
use crate::types::{Memory, MemoryId};

// ============================================================================
// Types and Enums
// ============================================================================

/// Type of conflict between memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictType {
    /// Direct contradiction in facts
    Contradiction,
    /// Outdated information
    Staleness,
    /// Duplicate content
    Duplicate,
    /// Semantic overlap
    SemanticOverlap,
    /// Inconsistent metadata
    MetadataInconsistency,
}

impl ConflictType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictType::Contradiction => "contradiction",
            ConflictType::Staleness => "staleness",
            ConflictType::Duplicate => "duplicate",
            ConflictType::SemanticOverlap => "semantic_overlap",
            ConflictType::MetadataInconsistency => "metadata_inconsistency",
        }
    }
}

impl std::str::FromStr for ConflictType {
    type Err = EngramError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "contradiction" => Ok(ConflictType::Contradiction),
            "staleness" => Ok(ConflictType::Staleness),
            "duplicate" => Ok(ConflictType::Duplicate),
            "semantic_overlap" => Ok(ConflictType::SemanticOverlap),
            "metadata_inconsistency" => Ok(ConflictType::MetadataInconsistency),
            _ => Err(EngramError::InvalidInput(format!(
                "Unknown conflict type: {}",
                s
            ))),
        }
    }
}

/// Severity of a conflict
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl ConflictSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictSeverity::Low => "low",
            ConflictSeverity::Medium => "medium",
            ConflictSeverity::High => "high",
            ConflictSeverity::Critical => "critical",
        }
    }
}

impl std::str::FromStr for ConflictSeverity {
    type Err = EngramError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "low" => Ok(ConflictSeverity::Low),
            "medium" => Ok(ConflictSeverity::Medium),
            "high" => Ok(ConflictSeverity::High),
            "critical" => Ok(ConflictSeverity::Critical),
            _ => Ok(ConflictSeverity::Medium),
        }
    }
}

/// Resolution type for conflicts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionType {
    /// Keep memory A, archive B
    KeepA,
    /// Keep memory B, archive A
    KeepB,
    /// Merge both into new memory
    Merge,
    /// Keep both as-is (mark as reviewed)
    KeepBoth,
    /// Delete both
    DeleteBoth,
    /// Mark as false positive
    FalsePositive,
}

impl ResolutionType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionType::KeepA => "keep_a",
            ResolutionType::KeepB => "keep_b",
            ResolutionType::Merge => "merge",
            ResolutionType::KeepBoth => "keep_both",
            ResolutionType::DeleteBoth => "delete_both",
            ResolutionType::FalsePositive => "false_positive",
        }
    }
}

/// Validation status for memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStatus {
    Unverified,
    Verified,
    Disputed,
    Stale,
}

impl ValidationStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ValidationStatus::Unverified => "unverified",
            ValidationStatus::Verified => "verified",
            ValidationStatus::Disputed => "disputed",
            ValidationStatus::Stale => "stale",
        }
    }
}

// ============================================================================
// Data Structures
// ============================================================================

/// A detected conflict between two memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConflict {
    pub id: i64,
    pub memory_a_id: MemoryId,
    pub memory_b_id: MemoryId,
    pub conflict_type: ConflictType,
    pub severity: ConflictSeverity,
    pub description: Option<String>,
    pub detected_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub resolution_type: Option<ResolutionType>,
    pub resolution_notes: Option<String>,
    pub auto_detected: bool,
}

/// A duplicate candidate pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateCandidate {
    pub id: i64,
    pub memory_a_id: MemoryId,
    pub memory_b_id: MemoryId,
    pub similarity_score: f32,
    pub similarity_type: String,
    pub detected_at: DateTime<Utc>,
    pub status: String,
}

/// Enhanced quality score with all components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedQualityScore {
    pub overall: f32,
    pub grade: char,
    pub clarity: f32,
    pub completeness: f32,
    pub freshness: f32,
    pub consistency: f32,
    pub source_trust: f32,
    pub suggestions: Vec<QualitySuggestion>,
    pub calculated_at: DateTime<Utc>,
}

/// A quality improvement suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualitySuggestion {
    pub category: String,
    pub priority: String,
    pub message: String,
    pub action: Option<String>,
}

/// Source trust score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceTrustScore {
    pub source_type: String,
    pub source_identifier: Option<String>,
    pub trust_score: f32,
    pub verification_count: i32,
    pub notes: Option<String>,
}

/// Quality report for a workspace or set of memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityReport {
    pub total_memories: i64,
    pub average_quality: f32,
    pub quality_distribution: HashMap<char, i64>,
    pub top_issues: Vec<QualityIssue>,
    pub conflicts_count: i64,
    pub duplicates_count: i64,
    pub suggestions_summary: Vec<String>,
    pub generated_at: DateTime<Utc>,
}

/// A quality issue in the report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityIssue {
    pub issue_type: String,
    pub count: i64,
    pub severity: String,
    pub description: String,
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for context quality analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextQualityConfig {
    /// Weight for clarity in quality score
    pub clarity_weight: f32,
    /// Weight for completeness
    pub completeness_weight: f32,
    /// Weight for freshness
    pub freshness_weight: f32,
    /// Weight for consistency
    pub consistency_weight: f32,
    /// Weight for source trust
    pub source_trust_weight: f32,
    /// Threshold for near-duplicate detection (0-1)
    pub duplicate_threshold: f32,
    /// Threshold for semantic similarity (0-1)
    pub semantic_threshold: f32,
    /// Days until memory is considered stale
    pub staleness_days: i64,
    /// Minimum content length for quality
    pub min_content_length: usize,
    /// Ideal content length
    pub ideal_content_length: usize,
}

impl Default for ContextQualityConfig {
    fn default() -> Self {
        Self {
            clarity_weight: 0.25,
            completeness_weight: 0.20,
            freshness_weight: 0.20,
            consistency_weight: 0.20,
            source_trust_weight: 0.15,
            duplicate_threshold: 0.85,
            semantic_threshold: 0.80,
            staleness_days: 90,
            min_content_length: 20,
            ideal_content_length: 200,
        }
    }
}

// ============================================================================
// Near-Duplicate Detection (ENG-48)
// ============================================================================

/// Calculate similarity between two strings using character n-grams
pub fn calculate_text_similarity(text_a: &str, text_b: &str) -> f32 {
    let ngram_size = 3;

    fn get_ngrams(text: &str, n: usize) -> HashSet<String> {
        let normalized: String = text
            .to_lowercase()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        if normalized.len() < n {
            return HashSet::new();
        }
        normalized
            .chars()
            .collect::<Vec<_>>()
            .windows(n)
            .map(|w| w.iter().collect::<String>())
            .collect()
    }

    let ngrams_a = get_ngrams(text_a, ngram_size);
    let ngrams_b = get_ngrams(text_b, ngram_size);

    if ngrams_a.is_empty() && ngrams_b.is_empty() {
        return 1.0;
    }
    if ngrams_a.is_empty() || ngrams_b.is_empty() {
        return 0.0;
    }

    let intersection = ngrams_a.intersection(&ngrams_b).count() as f32;
    let union = ngrams_a.union(&ngrams_b).count() as f32;

    intersection / union
}

/// Find near-duplicate memories using text similarity
pub fn find_near_duplicates(
    conn: &Connection,
    threshold: f32,
    limit: i64,
) -> Result<Vec<DuplicateCandidate>> {
    // Get memories that haven't been checked yet
    let mut stmt = conn.prepare(
        r#"
        SELECT id, content FROM memories
        WHERE deleted_at IS NULL
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )?;

    let memories: Vec<(i64, String)> = stmt
        .query_map(params![limit * 2], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut duplicates = Vec::new();

    // Compare pairs
    for i in 0..memories.len() {
        for j in (i + 1)..memories.len() {
            let (id_a, content_a) = &memories[i];
            let (id_b, content_b) = &memories[j];

            let similarity = calculate_text_similarity(content_a, content_b);

            if similarity >= threshold {
                // Check if already recorded
                let exists: bool = conn.query_row(
                    "SELECT 1 FROM duplicate_candidates WHERE memory_a_id = ? AND memory_b_id = ?",
                    params![id_a, id_b],
                    |_| Ok(true),
                ).unwrap_or(false);

                if !exists {
                    conn.execute(
                        r#"
                        INSERT OR IGNORE INTO duplicate_candidates
                        (memory_a_id, memory_b_id, similarity_score, similarity_type)
                        VALUES (?, ?, ?, 'content')
                        "#,
                        params![id_a, id_b, similarity],
                    )?;

                    duplicates.push(DuplicateCandidate {
                        id: 0,
                        memory_a_id: *id_a,
                        memory_b_id: *id_b,
                        similarity_score: similarity,
                        similarity_type: "content".to_string(),
                        detected_at: Utc::now(),
                        status: "pending".to_string(),
                    });
                }
            }
        }
    }

    Ok(duplicates)
}

/// Get pending duplicate candidates
pub fn get_pending_duplicates(conn: &Connection, limit: i64) -> Result<Vec<DuplicateCandidate>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, memory_a_id, memory_b_id, similarity_score, similarity_type, detected_at, status
        FROM duplicate_candidates
        WHERE status = 'pending'
        ORDER BY similarity_score DESC
        LIMIT ?
        "#,
    )?;

    let duplicates = stmt
        .query_map(params![limit], |row| {
            Ok(DuplicateCandidate {
                id: row.get(0)?,
                memory_a_id: row.get(1)?,
                memory_b_id: row.get(2)?,
                similarity_score: row.get(3)?,
                similarity_type: row.get(4)?,
                detected_at: row
                    .get::<_, String>(5)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
                status: row.get(6)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(duplicates)
}

// ============================================================================
// Semantic Deduplication (ENG-49)
// ============================================================================

/// Find semantic duplicates using embedding similarity
pub fn find_semantic_duplicates(
    conn: &Connection,
    query_embedding: &[f32],
    threshold: f32,
    limit: i64,
) -> Result<Vec<DuplicateCandidate>> {
    // Use existing embedding search infrastructure
    let mut stmt = conn.prepare(
        r#"
        SELECT m.id, e.embedding
        FROM memories m
        JOIN embeddings e ON m.id = e.memory_id
        WHERE m.deleted_at IS NULL
        LIMIT ?
        "#,
    )?;

    let memories: Vec<(i64, Vec<f32>)> = stmt
        .query_map(params![limit], |row| {
            let id: i64 = row.get(0)?;
            let embedding_blob: Vec<u8> = row.get(1)?;
            let embedding: Vec<f32> = embedding_blob
                .chunks(4)
                .map(|chunk| {
                    let bytes: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
                    f32::from_le_bytes(bytes)
                })
                .collect();
            Ok((id, embedding))
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut duplicates = Vec::new();

    for (id, embedding) in &memories {
        let similarity = cosine_similarity(query_embedding, embedding);
        if similarity >= threshold {
            duplicates.push(DuplicateCandidate {
                id: 0,
                memory_a_id: 0, // Query memory
                memory_b_id: *id,
                similarity_score: similarity,
                similarity_type: "semantic".to_string(),
                detected_at: Utc::now(),
                status: "pending".to_string(),
            });
        }
    }

    Ok(duplicates)
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }

    dot_product / (magnitude_a * magnitude_b)
}

// ============================================================================
// Conflict Detection (ENG-50)
// ============================================================================

/// Detect conflicts for a memory against existing memories
pub fn detect_conflicts(
    conn: &Connection,
    memory_id: MemoryId,
    config: &ContextQualityConfig,
) -> Result<Vec<MemoryConflict>> {
    let memory = get_memory(conn, memory_id)?;
    let mut conflicts = Vec::new();

    // Find memories with similar tags or content that might conflict
    let mut stmt = conn.prepare(
        r#"
        SELECT id, content, tags, updated_at
        FROM memories
        WHERE id != ? AND deleted_at IS NULL
        AND (
            -- Same workspace
            workspace = (SELECT workspace FROM memories WHERE id = ?)
            -- Or overlapping tags
            OR EXISTS (
                SELECT 1 FROM json_each(tags) t1
                WHERE t1.value IN (SELECT value FROM json_each((SELECT tags FROM memories WHERE id = ?)))
            )
        )
        LIMIT 100
        "#,
    )?;

    let candidates: Vec<(i64, String, String, String)> = stmt
        .query_map(params![memory_id, memory_id, memory_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .filter_map(|r| r.ok())
        .collect();

    for (other_id, other_content, _other_tags, other_updated) in candidates {
        // Check for staleness conflict
        let memory_date: DateTime<Utc> = memory.updated_at;
        let other_date: DateTime<Utc> = other_updated.parse().unwrap_or(memory_date);
        let days_diff = (memory_date - other_date).num_days().abs();

        if days_diff > config.staleness_days {
            // Check content similarity to see if they're about the same topic
            let similarity = calculate_text_similarity(&memory.content, &other_content);
            if similarity > 0.3 {
                let conflict = create_conflict(
                    conn,
                    memory_id,
                    other_id,
                    ConflictType::Staleness,
                    ConflictSeverity::Medium,
                    Some(format!(
                        "Memories differ by {} days and have {:.0}% content similarity",
                        days_diff,
                        similarity * 100.0
                    )),
                )?;
                conflicts.push(conflict);
            }
        }

        // Check for duplicate/overlap
        let similarity = calculate_text_similarity(&memory.content, &other_content);
        if similarity >= config.duplicate_threshold {
            let conflict = create_conflict(
                conn,
                memory_id,
                other_id,
                ConflictType::Duplicate,
                ConflictSeverity::High,
                Some(format!("Content similarity: {:.0}%", similarity * 100.0)),
            )?;
            conflicts.push(conflict);
        } else if similarity >= config.semantic_threshold {
            let conflict = create_conflict(
                conn,
                memory_id,
                other_id,
                ConflictType::SemanticOverlap,
                ConflictSeverity::Low,
                Some(format!("Semantic overlap: {:.0}%", similarity * 100.0)),
            )?;
            conflicts.push(conflict);
        }
    }

    Ok(conflicts)
}

/// Create a conflict record
fn create_conflict(
    conn: &Connection,
    memory_a_id: MemoryId,
    memory_b_id: MemoryId,
    conflict_type: ConflictType,
    severity: ConflictSeverity,
    description: Option<String>,
) -> Result<MemoryConflict> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    conn.execute(
        r#"
        INSERT OR IGNORE INTO memory_conflicts
        (memory_a_id, memory_b_id, conflict_type, severity, description, detected_at)
        VALUES (?, ?, ?, ?, ?, ?)
        "#,
        params![
            memory_a_id,
            memory_b_id,
            conflict_type.as_str(),
            severity.as_str(),
            description,
            now_str
        ],
    )?;

    let id = conn.last_insert_rowid();

    Ok(MemoryConflict {
        id,
        memory_a_id,
        memory_b_id,
        conflict_type,
        severity,
        description,
        detected_at: now,
        resolved_at: None,
        resolution_type: None,
        resolution_notes: None,
        auto_detected: true,
    })
}

/// Get unresolved conflicts
pub fn get_unresolved_conflicts(conn: &Connection, limit: i64) -> Result<Vec<MemoryConflict>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, memory_a_id, memory_b_id, conflict_type, severity, description,
               detected_at, resolved_at, resolution_type, resolution_notes, auto_detected
        FROM memory_conflicts
        WHERE resolved_at IS NULL
        ORDER BY
            CASE severity
                WHEN 'critical' THEN 1
                WHEN 'high' THEN 2
                WHEN 'medium' THEN 3
                ELSE 4
            END,
            detected_at DESC
        LIMIT ?
        "#,
    )?;

    let conflicts = stmt
        .query_map(params![limit], |row| {
            Ok(MemoryConflict {
                id: row.get(0)?,
                memory_a_id: row.get(1)?,
                memory_b_id: row.get(2)?,
                conflict_type: row
                    .get::<_, String>(3)?
                    .parse()
                    .unwrap_or(ConflictType::Contradiction),
                severity: row
                    .get::<_, String>(4)?
                    .parse()
                    .unwrap_or(ConflictSeverity::Medium),
                description: row.get(5)?,
                detected_at: row
                    .get::<_, String>(6)?
                    .parse()
                    .unwrap_or_else(|_| Utc::now()),
                resolved_at: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|s| s.parse().ok()),
                resolution_type: None,
                resolution_notes: row.get(9)?,
                auto_detected: row.get::<_, i32>(10)? == 1,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(conflicts)
}

// ============================================================================
// Contradiction Resolution (ENG-51)
// ============================================================================

/// Resolve a conflict between memories
pub fn resolve_conflict(
    conn: &Connection,
    conflict_id: i64,
    resolution_type: ResolutionType,
    notes: Option<&str>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"
        UPDATE memory_conflicts
        SET resolved_at = ?, resolution_type = ?, resolution_notes = ?
        WHERE id = ?
        "#,
        params![now, resolution_type.as_str(), notes, conflict_id],
    )?;

    // Apply resolution
    let (memory_a_id, memory_b_id): (i64, i64) = conn.query_row(
        "SELECT memory_a_id, memory_b_id FROM memory_conflicts WHERE id = ?",
        params![conflict_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    match resolution_type {
        ResolutionType::KeepA => {
            // Archive memory B
            conn.execute(
                "UPDATE memories SET lifecycle_state = 'archived' WHERE id = ?",
                params![memory_b_id],
            )?;
        }
        ResolutionType::KeepB => {
            // Archive memory A
            conn.execute(
                "UPDATE memories SET lifecycle_state = 'archived' WHERE id = ?",
                params![memory_a_id],
            )?;
        }
        ResolutionType::DeleteBoth => {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE memories SET deleted_at = ? WHERE id IN (?, ?)",
                params![now, memory_a_id, memory_b_id],
            )?;
        }
        _ => {
            // KeepBoth, Merge, FalsePositive - no automatic action
        }
    }

    Ok(())
}

// ============================================================================
// Enhanced Quality Scoring (ENG-52)
// ============================================================================

/// Calculate enhanced quality score for a memory
pub fn calculate_quality_score(
    conn: &Connection,
    memory_id: MemoryId,
    config: &ContextQualityConfig,
) -> Result<EnhancedQualityScore> {
    let memory = get_memory(conn, memory_id)?;

    let clarity = score_clarity(&memory);
    let completeness = score_completeness(&memory, config);
    let freshness = score_freshness(&memory, config);
    let consistency = score_consistency(conn, memory_id)?;
    let source_trust = get_source_trust_for_memory(conn, &memory)?;

    let overall = clarity * config.clarity_weight
        + completeness * config.completeness_weight
        + freshness * config.freshness_weight
        + consistency * config.consistency_weight
        + source_trust * config.source_trust_weight;

    let grade = match overall {
        s if s >= 0.9 => 'A',
        s if s >= 0.8 => 'B',
        s if s >= 0.7 => 'C',
        s if s >= 0.6 => 'D',
        _ => 'F',
    };

    let suggestions = generate_quality_suggestions(
        &memory,
        clarity,
        completeness,
        freshness,
        consistency,
        source_trust,
    );

    // Record in history
    let now = Utc::now();
    conn.execute(
        r#"
        INSERT INTO quality_history
        (memory_id, quality_score, clarity_score, completeness_score, freshness_score, consistency_score, source_trust_score)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        params![memory_id, overall, clarity, completeness, freshness, consistency, source_trust],
    )?;

    // Update memory quality score
    conn.execute(
        "UPDATE memories SET quality_score = ? WHERE id = ?",
        params![overall, memory_id],
    )?;

    Ok(EnhancedQualityScore {
        overall,
        grade,
        clarity,
        completeness,
        freshness,
        consistency,
        source_trust,
        suggestions,
        calculated_at: now,
    })
}

fn score_clarity(memory: &Memory) -> f32 {
    let content = &memory.content;
    let mut score: f32 = 0.5;

    // Sentence structure
    let sentence_count =
        content.matches('.').count() + content.matches('!').count() + content.matches('?').count();
    if sentence_count > 0 {
        score += 0.15;
    }

    // Word clarity
    let word_count = content.split_whitespace().count();
    if word_count > 0 {
        let avg_word_len: f32 = content
            .split_whitespace()
            .map(|w| w.len() as f32)
            .sum::<f32>()
            / word_count as f32;

        if (3.0..=10.0).contains(&avg_word_len) {
            score += 0.2;
        }
    }

    // Has organization (tags)
    if !memory.tags.is_empty() {
        score += 0.15;
    }

    score.min(1.0)
}

fn score_completeness(memory: &Memory, config: &ContextQualityConfig) -> f32 {
    let len = memory.content.len();

    if len < config.min_content_length {
        return 0.3;
    }

    if len >= config.ideal_content_length {
        return 1.0;
    }

    let range = (config.ideal_content_length - config.min_content_length) as f32;
    let progress = (len - config.min_content_length) as f32;
    0.3 + 0.7 * (progress / range)
}

fn score_freshness(memory: &Memory, config: &ContextQualityConfig) -> f32 {
    let age_days = (Utc::now() - memory.updated_at).num_days() as f32;
    let staleness = config.staleness_days as f32;

    if age_days <= 0.0 {
        1.0
    } else if age_days >= staleness {
        0.2
    } else {
        1.0 - 0.8 * (age_days / staleness)
    }
}

fn score_consistency(conn: &Connection, memory_id: MemoryId) -> Result<f32> {
    // Check for unresolved conflicts
    let conflict_count: i64 = conn.query_row(
        r#"
        SELECT COUNT(*) FROM memory_conflicts
        WHERE (memory_a_id = ? OR memory_b_id = ?) AND resolved_at IS NULL
        "#,
        params![memory_id, memory_id],
        |row| row.get(0),
    )?;

    Ok(match conflict_count {
        0 => 1.0,
        1 => 0.7,
        2 => 0.5,
        _ => 0.3,
    })
}

fn get_source_trust_for_memory(conn: &Connection, memory: &Memory) -> Result<f32> {
    // Determine source type from metadata
    let source_type = memory
        .metadata
        .get("origin")
        .and_then(|v| v.as_str())
        .unwrap_or("user");

    let trust_score: f32 = conn
        .query_row(
            "SELECT trust_score FROM source_trust_scores WHERE source_type = ?",
            params![source_type],
            |row| row.get(0),
        )
        .unwrap_or(0.7);

    Ok(trust_score)
}

fn generate_quality_suggestions(
    memory: &Memory,
    clarity: f32,
    completeness: f32,
    freshness: f32,
    consistency: f32,
    _source_trust: f32,
) -> Vec<QualitySuggestion> {
    let mut suggestions = Vec::new();

    if completeness < 0.5 {
        suggestions.push(QualitySuggestion {
            category: "completeness".to_string(),
            priority: "high".to_string(),
            message: "Add more detail to make this memory more useful".to_string(),
            action: Some("expand".to_string()),
        });
    }

    if clarity < 0.5 {
        suggestions.push(QualitySuggestion {
            category: "clarity".to_string(),
            priority: "medium".to_string(),
            message: "Consider adding structure with clear sentences".to_string(),
            action: Some("restructure".to_string()),
        });
    }

    if memory.tags.is_empty() {
        suggestions.push(QualitySuggestion {
            category: "organization".to_string(),
            priority: "low".to_string(),
            message: "Add tags to improve organization and searchability".to_string(),
            action: Some("add_tags".to_string()),
        });
    }

    if freshness < 0.3 {
        suggestions.push(QualitySuggestion {
            category: "freshness".to_string(),
            priority: "medium".to_string(),
            message: "This memory may be outdated - consider reviewing".to_string(),
            action: Some("review".to_string()),
        });
    }

    if consistency < 0.5 {
        suggestions.push(QualitySuggestion {
            category: "consistency".to_string(),
            priority: "high".to_string(),
            message: "This memory has unresolved conflicts - review and resolve".to_string(),
            action: Some("resolve_conflicts".to_string()),
        });
    }

    suggestions
}

// ============================================================================
// Quality Report (ENG-64)
// ============================================================================

/// Generate a quality report for a workspace
pub fn generate_quality_report(
    conn: &Connection,
    workspace: Option<&str>,
) -> Result<QualityReport> {
    let workspace_filter = workspace.unwrap_or("default");

    // Total memories
    let total_memories: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memories WHERE workspace = ? AND deleted_at IS NULL",
        params![workspace_filter],
        |row| row.get(0),
    )?;

    // Average quality
    let average_quality: f32 = conn
        .query_row(
            "SELECT COALESCE(AVG(quality_score), 0.5) FROM memories WHERE workspace = ? AND deleted_at IS NULL",
            params![workspace_filter],
            |row| row.get(0),
        )
        .unwrap_or(0.5);

    // Quality distribution
    let mut distribution = HashMap::new();
    let grades = ['A', 'B', 'C', 'D', 'F'];
    for grade in grades {
        let (min, max) = match grade {
            'A' => (0.9, 1.1),
            'B' => (0.8, 0.9),
            'C' => (0.7, 0.8),
            'D' => (0.6, 0.7),
            _ => (0.0, 0.6),
        };
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE workspace = ? AND deleted_at IS NULL AND quality_score >= ? AND quality_score < ?",
            params![workspace_filter, min, max],
            |row| row.get(0),
        ).unwrap_or(0);
        distribution.insert(grade, count);
    }

    // Conflicts count
    let conflicts_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_conflicts WHERE resolved_at IS NULL",
        [],
        |row| row.get(0),
    )?;

    // Duplicates count
    let duplicates_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM duplicate_candidates WHERE status = 'pending'",
        [],
        |row| row.get(0),
    )?;

    // Top issues
    let mut top_issues = Vec::new();

    if conflicts_count > 0 {
        top_issues.push(QualityIssue {
            issue_type: "conflicts".to_string(),
            count: conflicts_count,
            severity: "high".to_string(),
            description: format!("{} unresolved conflicts detected", conflicts_count),
        });
    }

    if duplicates_count > 0 {
        top_issues.push(QualityIssue {
            issue_type: "duplicates".to_string(),
            count: duplicates_count,
            severity: "medium".to_string(),
            description: format!("{} potential duplicates found", duplicates_count),
        });
    }

    // Low quality count
    let low_quality_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memories WHERE workspace = ? AND deleted_at IS NULL AND quality_score < 0.5",
        params![workspace_filter],
        |row| row.get(0),
    ).unwrap_or(0);

    if low_quality_count > 0 {
        top_issues.push(QualityIssue {
            issue_type: "low_quality".to_string(),
            count: low_quality_count,
            severity: "medium".to_string(),
            description: format!("{} memories with low quality scores", low_quality_count),
        });
    }

    let suggestions_summary = vec![
        format!("Average quality score: {:.0}%", average_quality * 100.0),
        format!("Total memories: {}", total_memories),
        if conflicts_count > 0 {
            format!(
                "Resolve {} conflicts to improve consistency",
                conflicts_count
            )
        } else {
            "No conflicts detected".to_string()
        },
    ];

    Ok(QualityReport {
        total_memories,
        average_quality,
        quality_distribution: distribution,
        top_issues,
        conflicts_count,
        duplicates_count,
        suggestions_summary,
        generated_at: Utc::now(),
    })
}

// ============================================================================
// Source Trust (ENG-53)
// ============================================================================

/// Get or set source trust score
pub fn get_source_trust(
    conn: &Connection,
    source_type: &str,
    source_identifier: Option<&str>,
) -> Result<SourceTrustScore> {
    let identifier = source_identifier.unwrap_or("default");

    let result = conn.query_row(
        r#"
        SELECT source_type, source_identifier, trust_score, verification_count, notes
        FROM source_trust_scores
        WHERE source_type = ? AND (source_identifier = ? OR source_identifier IS NULL)
        ORDER BY source_identifier DESC
        LIMIT 1
        "#,
        params![source_type, identifier],
        |row| {
            Ok(SourceTrustScore {
                source_type: row.get(0)?,
                source_identifier: row.get(1)?,
                trust_score: row.get(2)?,
                verification_count: row.get(3)?,
                notes: row.get(4)?,
            })
        },
    );

    result.map_err(|_| EngramError::NotFound(0))
}

/// Update source trust score
pub fn update_source_trust(
    conn: &Connection,
    source_type: &str,
    source_identifier: Option<&str>,
    trust_score: f32,
    notes: Option<&str>,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"
        INSERT INTO source_trust_scores (source_type, source_identifier, trust_score, notes, updated_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(source_type, source_identifier)
        DO UPDATE SET trust_score = ?, notes = ?, updated_at = ?
        "#,
        params![
            source_type,
            source_identifier,
            trust_score,
            notes,
            now,
            trust_score,
            notes,
            now
        ],
    )?;

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_similarity() {
        let a = "The quick brown fox jumps over the lazy dog";
        let b = "The quick brown fox jumps over the lazy cat";
        let c = "Something completely different";

        let sim_ab = calculate_text_similarity(a, b);
        let sim_ac = calculate_text_similarity(a, c);

        assert!(sim_ab > 0.8, "Similar texts should have high similarity");
        assert!(sim_ac < 0.3, "Different texts should have low similarity");
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];

        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }

    #[test]
    fn test_conflict_type_parsing() {
        assert_eq!(
            "contradiction".parse::<ConflictType>().unwrap(),
            ConflictType::Contradiction
        );
        assert_eq!(
            "duplicate".parse::<ConflictType>().unwrap(),
            ConflictType::Duplicate
        );
    }
}
