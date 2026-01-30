//! Salience Scoring System (Phase 8 - ENG-66, ENG-67, ENG-68)
//!
//! Calculates dynamic salience scores for memories based on:
//! - Recency: How recently the memory was accessed (exponential decay)
//! - Frequency: How often the memory is accessed (log-scaled)
//! - Importance: User-set importance value
//! - Feedback: Explicit user signals (boost/demote)
//!
//! Salience is used for:
//! - Search result reranking
//! - Priority queue ordering
//! - Automatic archival decisions
//! - Context budget allocation

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::types::{LifecycleState, Memory, MemoryId};

/// Configuration for salience scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalienceConfig {
    /// Weight for recency component (0.0 - 1.0)
    pub recency_weight: f32,
    /// Weight for frequency component (0.0 - 1.0)
    pub frequency_weight: f32,
    /// Weight for importance component (0.0 - 1.0)
    pub importance_weight: f32,
    /// Weight for feedback component (0.0 - 1.0)
    pub feedback_weight: f32,
    /// Half-life for recency decay in days
    pub recency_half_life_days: f32,
    /// Log base for frequency scaling
    pub frequency_log_base: f32,
    /// Maximum frequency count for scaling (diminishing returns)
    pub frequency_max_count: i32,
    /// Minimum salience score (floor)
    pub min_salience: f32,
    /// Days of inactivity before marking stale
    pub stale_threshold_days: i64,
    /// Days of inactivity before suggesting archive
    pub archive_threshold_days: i64,
}

impl Default for SalienceConfig {
    fn default() -> Self {
        Self {
            recency_weight: 0.30,
            frequency_weight: 0.20,
            importance_weight: 0.30,
            feedback_weight: 0.20,
            recency_half_life_days: 14.0, // 2 weeks half-life
            frequency_log_base: 2.0,
            frequency_max_count: 100,
            min_salience: 0.05,
            stale_threshold_days: 30,
            archive_threshold_days: 90,
        }
    }
}

/// Salience score with component breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalienceScore {
    /// Overall salience score (0.0 - 1.0)
    pub score: f32,
    /// Recency component score
    pub recency: f32,
    /// Frequency component score
    pub frequency: f32,
    /// Importance component score
    pub importance: f32,
    /// Feedback component score
    pub feedback: f32,
    /// When the score was calculated
    pub calculated_at: DateTime<Utc>,
    /// Suggested lifecycle state based on salience
    pub suggested_state: LifecycleState,
}

/// Result of a decay/refresh operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecayResult {
    /// Number of memories processed
    pub processed: i64,
    /// Number of memories marked stale
    pub marked_stale: i64,
    /// Number of memories suggested for archive
    pub suggested_archive: i64,
    /// Number of salience history records created
    pub history_records: i64,
    /// Duration of the operation in milliseconds
    pub duration_ms: i64,
}

/// Salience statistics for analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalienceStats {
    /// Total memories analyzed
    pub total_memories: i64,
    /// Average salience score
    pub mean_salience: f32,
    /// Median salience score
    pub median_salience: f32,
    /// Standard deviation
    pub std_dev: f32,
    /// Percentile distribution (10th, 25th, 50th, 75th, 90th)
    pub percentiles: SaliencePercentiles,
    /// Count by lifecycle state
    pub by_state: StateDistribution,
    /// Low salience memories (candidates for archive)
    pub low_salience_count: i64,
    /// High salience memories (most relevant)
    pub high_salience_count: i64,
}

/// Percentile distribution for salience scores
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaliencePercentiles {
    pub p10: f32,
    pub p25: f32,
    pub p50: f32,
    pub p75: f32,
    pub p90: f32,
}

/// Distribution of memories by lifecycle state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDistribution {
    pub active: i64,
    pub stale: i64,
    pub archived: i64,
}

/// Memory with salience score for priority queue
#[derive(Debug, Clone)]
pub struct ScoredMemory {
    pub memory: Memory,
    pub salience: SalienceScore,
}

impl PartialEq for ScoredMemory {
    fn eq(&self, other: &Self) -> bool {
        self.memory.id == other.memory.id
    }
}

impl Eq for ScoredMemory {}

impl PartialOrd for ScoredMemory {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScoredMemory {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Higher salience = higher priority
        self.salience
            .score
            .partial_cmp(&other.salience.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .reverse() // For max-heap behavior in BinaryHeap
    }
}

/// Salience calculator engine
pub struct SalienceCalculator {
    config: SalienceConfig,
}

impl Default for SalienceCalculator {
    fn default() -> Self {
        Self::new(SalienceConfig::default())
    }
}

impl SalienceCalculator {
    /// Create a new salience calculator with custom config
    pub fn new(config: SalienceConfig) -> Self {
        Self { config }
    }

    /// Calculate salience score for a single memory
    pub fn calculate(&self, memory: &Memory, feedback_signal: f32) -> SalienceScore {
        let now = Utc::now();

        // Recency: exponential decay based on last access
        let recency = self.calculate_recency(memory, now);

        // Frequency: log-scaled access count
        let frequency = self.calculate_frequency(memory);

        // Importance: user-set value (already 0.0-1.0)
        let importance = memory.importance;

        // Feedback: explicit user signal (clamped to 0.0-1.0)
        let feedback = feedback_signal.clamp(0.0, 1.0);

        // Weighted combination
        let score = (recency * self.config.recency_weight
            + frequency * self.config.frequency_weight
            + importance * self.config.importance_weight
            + feedback * self.config.feedback_weight)
            .max(self.config.min_salience)
            .min(1.0);

        // Suggest lifecycle state based on score and age
        let suggested_state = self.suggest_lifecycle_state(memory, score, now);

        SalienceScore {
            score,
            recency,
            frequency,
            importance,
            feedback,
            calculated_at: now,
            suggested_state,
        }
    }

    /// Calculate recency score using exponential decay
    fn calculate_recency(&self, memory: &Memory, now: DateTime<Utc>) -> f32 {
        let last_access = memory.last_accessed_at.unwrap_or(memory.created_at);
        let days_since_access = (now - last_access).num_hours() as f32 / 24.0;

        // Exponential decay: score = 0.5^(days / half_life)
        let decay = 0.5_f32.powf(days_since_access / self.config.recency_half_life_days);

        decay.max(0.0).min(1.0)
    }

    /// Calculate frequency score using log scaling
    fn calculate_frequency(&self, memory: &Memory) -> f32 {
        let count = memory.access_count.max(0) as f32;
        let max_count = self.config.frequency_max_count as f32;

        if count <= 0.0 {
            return 0.1; // Base score for never-accessed
        }

        // Log scaling with diminishing returns
        // log_b(x+1) / log_b(max+1) gives 0-1 range
        let log_base = self.config.frequency_log_base;
        let log_count = (count + 1.0).log(log_base);
        let log_max = (max_count + 1.0).log(log_base);

        (log_count / log_max).min(1.0)
    }

    /// Suggest lifecycle state based on salience and age
    fn suggest_lifecycle_state(
        &self,
        memory: &Memory,
        score: f32,
        now: DateTime<Utc>,
    ) -> LifecycleState {
        let last_access = memory.last_accessed_at.unwrap_or(memory.created_at);
        let days_inactive = (now - last_access).num_days();

        // Already archived stays archived
        if memory.lifecycle_state == LifecycleState::Archived {
            return LifecycleState::Archived;
        }

        // Low salience + long inactivity = suggest archive
        if score < 0.2 && days_inactive >= self.config.archive_threshold_days {
            return LifecycleState::Archived;
        }

        // Medium-low salience or moderate inactivity = stale
        if score < 0.4 || days_inactive >= self.config.stale_threshold_days {
            return LifecycleState::Stale;
        }

        LifecycleState::Active
    }

    /// Calculate salience for multiple memories
    pub fn calculate_batch(
        &self,
        memories: &[Memory],
        feedback_signals: Option<&HashMap<MemoryId, f32>>,
    ) -> Vec<ScoredMemory> {
        let empty = HashMap::new();
        let signals = feedback_signals.unwrap_or(&empty);

        memories
            .iter()
            .map(|m| {
                let feedback = signals.get(&m.id).copied().unwrap_or(0.5);
                ScoredMemory {
                    salience: self.calculate(m, feedback),
                    memory: m.clone(),
                }
            })
            .collect()
    }

    /// Get salience-sorted priority queue of memories
    pub fn priority_queue(&self, memories: &[Memory]) -> Vec<ScoredMemory> {
        let mut scored = self.calculate_batch(memories, None);
        scored.sort_by(|a, b| {
            b.salience
                .score
                .partial_cmp(&a.salience.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored
    }
}

use std::collections::HashMap;

/// Run decay on all memories and update lifecycle states
pub fn run_salience_decay(
    conn: &Connection,
    config: &SalienceConfig,
    record_history: bool,
) -> Result<DecayResult> {
    run_salience_decay_in_workspace(conn, config, record_history, None)
}

pub fn run_salience_decay_in_workspace(
    conn: &Connection,
    config: &SalienceConfig,
    record_history: bool,
    workspace: Option<&str>,
) -> Result<DecayResult> {
    let start = std::time::Instant::now();
    let now = Utc::now();
    let now_str = now.to_rfc3339();
    let _calculator = SalienceCalculator::new(config.clone());

    // Get all non-archived memories
    let memories: Vec<(MemoryId, f32, i32, String, String, Option<String>, String)> =
        if let Some(workspace) = workspace {
            let mut stmt = conn.prepare(
                "SELECT id, content, memory_type, importance, access_count,
                        created_at, updated_at, last_accessed_at, lifecycle_state,
                        workspace, tier
                 FROM memories
                 WHERE lifecycle_state != 'archived'
                 AND (expires_at IS NULL OR expires_at > ?)
                 AND workspace = ?",
            )?;

            let rows = stmt.query_map(params![now_str, workspace], |row| {
                Ok((
                    row.get::<_, MemoryId>(0)?,
                    row.get::<_, f32>(3)?,            // importance
                    row.get::<_, i32>(4)?,            // access_count
                    row.get::<_, String>(5)?,         // created_at
                    row.get::<_, String>(6)?,         // updated_at
                    row.get::<_, Option<String>>(7)?, // last_accessed_at
                    row.get::<_, String>(8)?,         // lifecycle_state
                ))
            })?;

            rows.collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, content, memory_type, importance, access_count,
                        created_at, updated_at, last_accessed_at, lifecycle_state,
                        workspace, tier
                 FROM memories
                 WHERE lifecycle_state != 'archived'
                 AND (expires_at IS NULL OR expires_at > ?)",
            )?;

            let rows = stmt.query_map(params![now_str], |row| {
                Ok((
                    row.get::<_, MemoryId>(0)?,
                    row.get::<_, f32>(3)?,            // importance
                    row.get::<_, i32>(4)?,            // access_count
                    row.get::<_, String>(5)?,         // created_at
                    row.get::<_, String>(6)?,         // updated_at
                    row.get::<_, Option<String>>(7)?, // last_accessed_at
                    row.get::<_, String>(8)?,         // lifecycle_state
                ))
            })?;

            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };

    let mut processed = 0i64;
    let mut marked_stale = 0i64;
    let mut suggested_archive = 0i64;
    let mut history_records = 0i64;

    for (
        id,
        importance,
        access_count,
        created_at_str,
        _updated_at_str,
        last_accessed_str,
        current_state,
    ) in memories
    {
        // Parse dates
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(now);

        let last_accessed_at = last_accessed_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        });

        // Calculate recency
        let last_access = last_accessed_at.unwrap_or(created_at);
        let days_since_access = (now - last_access).num_hours() as f32 / 24.0;
        let recency = 0.5_f32.powf(days_since_access / config.recency_half_life_days);

        // Calculate frequency
        let count = access_count.max(0) as f32;
        let frequency = if count <= 0.0 {
            0.1
        } else {
            let log_count = (count + 1.0).log(config.frequency_log_base);
            let log_max = (config.frequency_max_count as f32 + 1.0).log(config.frequency_log_base);
            (log_count / log_max).min(1.0)
        };

        // Calculate overall score (using 0.5 as default feedback)
        let score = (recency * config.recency_weight
            + frequency * config.frequency_weight
            + importance * config.importance_weight
            + 0.5 * config.feedback_weight)
            .max(config.min_salience)
            .min(1.0);

        // Determine suggested state
        let days_inactive = (now - last_access).num_days();
        let new_state = if score < 0.2 && days_inactive >= config.archive_threshold_days {
            "archived"
        } else if score < 0.4 || days_inactive >= config.stale_threshold_days {
            "stale"
        } else {
            "active"
        };

        // Update state if changed
        if new_state != current_state {
            conn.execute(
                "UPDATE memories SET lifecycle_state = ?, updated_at = ? WHERE id = ?",
                params![new_state, now_str, id],
            )?;

            if new_state == "stale" {
                marked_stale += 1;
            } else if new_state == "archived" {
                suggested_archive += 1;
            }
        }

        // Record history if enabled
        if record_history {
            conn.execute(
                "INSERT INTO salience_history (memory_id, salience_score, recency_score,
                 frequency_score, importance_score, feedback_score, recorded_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, score, recency, frequency, importance, 0.5, now_str],
            )?;
            history_records += 1;
        }

        processed += 1;
    }

    let duration_ms = start.elapsed().as_millis() as i64;

    Ok(DecayResult {
        processed,
        marked_stale,
        suggested_archive,
        history_records,
        duration_ms,
    })
}

/// Get salience score for a specific memory
pub fn get_memory_salience(
    conn: &Connection,
    memory_id: MemoryId,
    config: &SalienceConfig,
) -> Result<Option<SalienceScore>> {
    get_memory_salience_with_feedback(conn, memory_id, config, 0.5)
}

pub fn get_memory_salience_with_feedback(
    conn: &Connection,
    memory_id: MemoryId,
    config: &SalienceConfig,
    feedback_signal: f32,
) -> Result<Option<SalienceScore>> {
    let row = conn.query_row(
        "SELECT importance, access_count, created_at, updated_at,
                last_accessed_at, lifecycle_state
         FROM memories WHERE id = ?",
        params![memory_id],
        |row| {
            Ok((
                row.get::<_, f32>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        },
    );

    match row {
        Ok((
            importance,
            access_count,
            created_at_str,
            _updated_at_str,
            last_accessed_str,
            lifecycle_str,
        )) => {
            let now = Utc::now();
            let calculator = SalienceCalculator::new(config.clone());

            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);

            let last_accessed_at = last_accessed_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });

            let lifecycle_state = lifecycle_str.parse().unwrap_or(LifecycleState::Active);

            // Create a minimal memory for calculation
            let memory = Memory {
                id: memory_id,
                content: String::new(),
                memory_type: crate::types::MemoryType::Note,
                tags: vec![],
                metadata: HashMap::new(),
                importance,
                access_count,
                created_at,
                updated_at: now,
                last_accessed_at,
                owner_id: None,
                visibility: crate::types::Visibility::Private,
                scope: crate::types::MemoryScope::Global,
                workspace: "default".to_string(),
                tier: crate::types::MemoryTier::Permanent,
                version: 1,
                has_embedding: false,
                expires_at: None,
                content_hash: None,
                event_time: None,
                event_duration_seconds: None,
                trigger_pattern: None,
                procedure_success_count: 0,
                procedure_failure_count: 0,
                summary_of_id: None,
                lifecycle_state,
            };

            Ok(Some(calculator.calculate(&memory, feedback_signal)))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Update importance (user feedback signal) for a memory
pub fn set_memory_importance(
    conn: &Connection,
    memory_id: MemoryId,
    importance: f32,
) -> Result<()> {
    let importance = importance.clamp(0.0, 1.0);
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "UPDATE memories SET importance = ?, updated_at = ? WHERE id = ?",
        params![importance, now, memory_id],
    )?;

    Ok(())
}

/// Boost a memory's importance (positive feedback)
pub fn boost_memory_salience(
    conn: &Connection,
    memory_id: MemoryId,
    boost_amount: f32,
) -> Result<f32> {
    let now = Utc::now().to_rfc3339();
    let boost = boost_amount.clamp(0.0, 0.5); // Max boost of 0.5

    // Update and return new importance
    conn.execute(
        "UPDATE memories SET importance = MIN(1.0, importance + ?), updated_at = ? WHERE id = ?",
        params![boost, now, memory_id],
    )?;

    let new_importance: f32 = conn.query_row(
        "SELECT importance FROM memories WHERE id = ?",
        params![memory_id],
        |row| row.get(0),
    )?;

    Ok(new_importance)
}

/// Demote a memory's importance (negative feedback)
pub fn demote_memory_salience(
    conn: &Connection,
    memory_id: MemoryId,
    demote_amount: f32,
) -> Result<f32> {
    let now = Utc::now().to_rfc3339();
    let demote = demote_amount.clamp(0.0, 0.5); // Max demote of 0.5

    conn.execute(
        "UPDATE memories SET importance = MAX(0.0, importance - ?), updated_at = ? WHERE id = ?",
        params![demote, now, memory_id],
    )?;

    let new_importance: f32 = conn.query_row(
        "SELECT importance FROM memories WHERE id = ?",
        params![memory_id],
        |row| row.get(0),
    )?;

    Ok(new_importance)
}

/// Get salience statistics for analytics
pub fn get_salience_stats(conn: &Connection, config: &SalienceConfig) -> Result<SalienceStats> {
    get_salience_stats_in_workspace(conn, config, None)
}

pub fn get_salience_stats_in_workspace(
    conn: &Connection,
    config: &SalienceConfig,
    workspace: Option<&str>,
) -> Result<SalienceStats> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    // Get all non-expired memories with their scores
    let mut scores: Vec<f32> = Vec::new();
    let mut active_count = 0i64;
    let mut stale_count = 0i64;
    let mut archived_count = 0i64;

    let rows = if let Some(workspace) = workspace {
        let mut stmt = conn.prepare(
            "SELECT importance, access_count, created_at, last_accessed_at, lifecycle_state
             FROM memories
             WHERE (expires_at IS NULL OR expires_at > ?)
             AND workspace = ?",
        )?;
        let rows = stmt.query_map(params![now_str, workspace], |row| {
            Ok((
                row.get::<_, f32>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    } else {
        let mut stmt = conn.prepare(
            "SELECT importance, access_count, created_at, last_accessed_at, lifecycle_state
             FROM memories
             WHERE (expires_at IS NULL OR expires_at > ?)",
        )?;
        let rows = stmt.query_map(params![now_str], |row| {
            Ok((
                row.get::<_, f32>(0)?,
                row.get::<_, i32>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()?
    };

    for (importance, access_count, created_at_str, last_accessed_str, state_str) in rows {

        // Calculate score
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(now);

        let last_access = last_accessed_str
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(created_at);

        let days_since_access = (now - last_access).num_hours() as f32 / 24.0;
        let recency = 0.5_f32.powf(days_since_access / config.recency_half_life_days);

        let count = access_count.max(0) as f32;
        let frequency = if count <= 0.0 {
            0.1
        } else {
            let log_count = (count + 1.0).log(config.frequency_log_base);
            let log_max = (config.frequency_max_count as f32 + 1.0).log(config.frequency_log_base);
            (log_count / log_max).min(1.0)
        };

        let score = (recency * config.recency_weight
            + frequency * config.frequency_weight
            + importance * config.importance_weight
            + 0.5 * config.feedback_weight)
            .max(config.min_salience)
            .min(1.0);

        scores.push(score);

        // Count by state
        match state_str.as_str() {
            "active" => active_count += 1,
            "stale" => stale_count += 1,
            "archived" => archived_count += 1,
            _ => active_count += 1,
        }
    }

    if scores.is_empty() {
        return Ok(SalienceStats {
            total_memories: 0,
            mean_salience: 0.0,
            median_salience: 0.0,
            std_dev: 0.0,
            percentiles: SaliencePercentiles {
                p10: 0.0,
                p25: 0.0,
                p50: 0.0,
                p75: 0.0,
                p90: 0.0,
            },
            by_state: StateDistribution {
                active: 0,
                stale: 0,
                archived: 0,
            },
            low_salience_count: 0,
            high_salience_count: 0,
        });
    }

    // Sort for percentile calculation
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let total = scores.len();
    let mean: f32 = scores.iter().sum::<f32>() / total as f32;
    let median = scores[total / 2];

    // Standard deviation
    let variance: f32 = scores.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / total as f32;
    let std_dev = variance.sqrt();

    // Percentiles
    let p10 = scores[(total as f32 * 0.10) as usize];
    let p25 = scores[(total as f32 * 0.25) as usize];
    let p50 = scores[(total as f32 * 0.50) as usize];
    let p75 = scores[((total as f32 * 0.75) as usize).min(total - 1)];
    let p90 = scores[((total as f32 * 0.90) as usize).min(total - 1)];

    // Count low/high salience
    let low_salience_count = scores.iter().filter(|&&s| s < 0.3).count() as i64;
    let high_salience_count = scores.iter().filter(|&&s| s > 0.7).count() as i64;

    Ok(SalienceStats {
        total_memories: total as i64,
        mean_salience: mean,
        median_salience: median,
        std_dev,
        percentiles: SaliencePercentiles {
            p10,
            p25,
            p50,
            p75,
            p90,
        },
        by_state: StateDistribution {
            active: active_count,
            stale: stale_count,
            archived: archived_count,
        },
        low_salience_count,
        high_salience_count,
    })
}

/// Get salience history for a memory (for trend analysis)
pub fn get_salience_history(
    conn: &Connection,
    memory_id: MemoryId,
    limit: i64,
) -> Result<Vec<SalienceHistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT salience_score, recency_score, frequency_score,
                importance_score, feedback_score, recorded_at
         FROM salience_history
         WHERE memory_id = ?
         ORDER BY recorded_at DESC
         LIMIT ?",
    )?;

    let entries = stmt
        .query_map(params![memory_id, limit], |row| {
            Ok(SalienceHistoryEntry {
                salience_score: row.get(0)?,
                recency_score: row.get(1)?,
                frequency_score: row.get(2)?,
                importance_score: row.get(3)?,
                feedback_score: row.get(4)?,
                recorded_at: row.get(5)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(entries)
}

/// Salience history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SalienceHistoryEntry {
    pub salience_score: f32,
    pub recency_score: f32,
    pub frequency_score: f32,
    pub importance_score: f32,
    pub feedback_score: f32,
    pub recorded_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_memory(
        id: MemoryId,
        importance: f32,
        access_count: i32,
        days_since_access: i64,
    ) -> Memory {
        let now = Utc::now();
        Memory {
            id,
            content: "Test content".to_string(),
            memory_type: crate::types::MemoryType::Note,
            tags: vec![],
            metadata: HashMap::new(),
            importance,
            access_count,
            created_at: now - chrono::Duration::days(30),
            updated_at: now - chrono::Duration::days(1),
            last_accessed_at: Some(now - chrono::Duration::days(days_since_access)),
            owner_id: None,
            visibility: crate::types::Visibility::Private,
            scope: crate::types::MemoryScope::Global,
            workspace: "default".to_string(),
            tier: crate::types::MemoryTier::Permanent,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None,
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            procedure_success_count: 0,
            procedure_failure_count: 0,
            summary_of_id: None,
            lifecycle_state: LifecycleState::Active,
        }
    }

    #[test]
    fn test_recency_decay() {
        let calculator = SalienceCalculator::default();

        // Recently accessed should have high recency
        let recent = create_test_memory(1, 0.5, 10, 0);
        let score_recent = calculator.calculate(&recent, 0.5);
        assert!(score_recent.recency > 0.9, "Recent should be > 0.9");

        // Accessed 14 days ago (half-life) should be ~0.5
        let half_life = create_test_memory(2, 0.5, 10, 14);
        let score_half = calculator.calculate(&half_life, 0.5);
        assert!(
            (score_half.recency - 0.5).abs() < 0.1,
            "Half-life should be ~0.5, got {}",
            score_half.recency
        );

        // Accessed 28 days ago (2x half-life) should be ~0.25
        let old = create_test_memory(3, 0.5, 10, 28);
        let score_old = calculator.calculate(&old, 0.5);
        assert!(
            (score_old.recency - 0.25).abs() < 0.1,
            "2x half-life should be ~0.25, got {}",
            score_old.recency
        );
    }

    #[test]
    fn test_frequency_scaling() {
        let calculator = SalienceCalculator::default();

        // Never accessed should have low frequency
        let never = create_test_memory(1, 0.5, 0, 1);
        let score_never = calculator.calculate(&never, 0.5);
        assert!(
            score_never.frequency < 0.2,
            "Never accessed should be < 0.2"
        );

        // Frequently accessed should have high frequency
        let frequent = create_test_memory(2, 0.5, 50, 1);
        let score_frequent = calculator.calculate(&frequent, 0.5);
        assert!(
            score_frequent.frequency > 0.6,
            "Frequently accessed should be > 0.6"
        );

        // Frequency should have diminishing returns
        let very_frequent = create_test_memory(3, 0.5, 100, 1);
        let score_very = calculator.calculate(&very_frequent, 0.5);
        assert!(
            score_very.frequency <= 1.0,
            "Max frequency should be <= 1.0"
        );
    }

    #[test]
    fn test_importance_weight() {
        let calculator = SalienceCalculator::default();

        let low_importance = create_test_memory(1, 0.1, 10, 1);
        let high_importance = create_test_memory(2, 0.9, 10, 1);

        let score_low = calculator.calculate(&low_importance, 0.5);
        let score_high = calculator.calculate(&high_importance, 0.5);

        assert!(
            score_high.score > score_low.score,
            "High importance should have higher salience"
        );
    }

    #[test]
    fn test_lifecycle_suggestion() {
        let calculator = SalienceCalculator::default();

        // Recent with good engagement = active
        let active = create_test_memory(1, 0.8, 20, 5);
        let score_active = calculator.calculate(&active, 0.5);
        assert_eq!(score_active.suggested_state, LifecycleState::Active);

        // Old with low engagement = stale
        let stale = create_test_memory(2, 0.3, 2, 45);
        let score_stale = calculator.calculate(&stale, 0.5);
        assert_eq!(score_stale.suggested_state, LifecycleState::Stale);

        // Very old with very low engagement = archived
        let archived = create_test_memory(3, 0.1, 0, 100);
        let score_archived = calculator.calculate(&archived, 0.1);
        assert_eq!(score_archived.suggested_state, LifecycleState::Archived);
    }

    #[test]
    fn test_priority_queue() {
        let calculator = SalienceCalculator::default();

        let memories = vec![
            create_test_memory(1, 0.3, 5, 20),  // Low salience
            create_test_memory(2, 0.9, 50, 1),  // High salience
            create_test_memory(3, 0.5, 10, 10), // Medium salience
        ];

        let queue = calculator.priority_queue(&memories);

        // Should be sorted by salience descending
        assert_eq!(queue[0].memory.id, 2, "Highest salience first");
        assert_eq!(queue[2].memory.id, 1, "Lowest salience last");
    }

    #[test]
    fn test_score_bounds() {
        let calculator = SalienceCalculator::default();

        // Test extreme cases
        let worst = create_test_memory(1, 0.0, 0, 365);
        let best = create_test_memory(2, 1.0, 100, 0);

        let score_worst = calculator.calculate(&worst, 0.0);
        let score_best = calculator.calculate(&best, 1.0);

        // Scores should be bounded 0.0-1.0
        assert!(score_worst.score >= 0.0 && score_worst.score <= 1.0);
        assert!(score_best.score >= 0.0 && score_best.score <= 1.0);

        // Min salience should be enforced
        assert!(score_worst.score >= 0.05, "Min salience should be enforced");
    }
}
