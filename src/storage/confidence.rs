//! Confidence decay for cross-references (RML-897)
//!
//! Relations automatically decay in confidence over time using exponential decay.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};

use crate::error::Result;
use crate::types::MemoryId;

/// Default half-life in days (configurable via env)
pub const DEFAULT_HALF_LIFE_DAYS: f32 = 30.0;

/// Calculate decayed confidence based on age
///
/// Uses exponential decay: confidence = initial * 0.5^(age_days / half_life)
pub fn calculate_decayed_confidence(
    initial_confidence: f32,
    created_at: DateTime<Utc>,
    half_life_days: f32,
) -> f32 {
    let age_days = (Utc::now() - created_at).num_days() as f32;
    initial_confidence * 0.5_f32.powf(age_days / half_life_days)
}

/// Get effective confidence for a cross-reference (considering decay and pinned status)
pub fn get_effective_confidence(
    conn: &Connection,
    from_id: MemoryId,
    to_id: MemoryId,
    half_life_days: f32,
) -> Result<Option<f32>> {
    let row = conn.query_row(
        "SELECT confidence, created_at, pinned FROM crossrefs
         WHERE from_id = ? AND to_id = ? AND valid_to IS NULL",
        params![from_id, to_id],
        |row| {
            let confidence: f32 = row.get(0)?;
            let created_at: String = row.get(1)?;
            let pinned: i32 = row.get(2)?;
            Ok((confidence, created_at, pinned != 0))
        },
    );

    match row {
        Ok((confidence, created_at_str, pinned)) => {
            if pinned {
                // Pinned relations don't decay
                return Ok(Some(confidence));
            }

            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Some(calculate_decayed_confidence(
                confidence,
                created_at,
                half_life_days,
            )))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all cross-references with decayed confidence scores
pub fn get_related_with_decay(
    conn: &Connection,
    memory_id: MemoryId,
    half_life_days: f32,
    min_confidence: f32,
) -> Result<Vec<DecayedCrossRef>> {
    let mut stmt = conn.prepare(
        "SELECT from_id, to_id, edge_type, score, confidence, strength,
                created_at, pinned
         FROM crossrefs
         WHERE (from_id = ? OR to_id = ?) AND valid_to IS NULL",
    )?;

    let now = Utc::now();
    let mut results = Vec::new();

    let rows = stmt.query_map(params![memory_id, memory_id], |row| {
        let from_id: MemoryId = row.get(0)?;
        let to_id: MemoryId = row.get(1)?;
        let edge_type: String = row.get(2)?;
        let score: f32 = row.get(3)?;
        let confidence: f32 = row.get(4)?;
        let strength: f32 = row.get(5)?;
        let created_at: String = row.get(6)?;
        let pinned: i32 = row.get(7)?;

        Ok((
            from_id,
            to_id,
            edge_type,
            score,
            confidence,
            strength,
            created_at,
            pinned != 0,
        ))
    })?;

    for row in rows {
        let (from_id, to_id, edge_type, score, confidence, strength, created_at_str, pinned) = row?;

        let effective_confidence = if pinned {
            confidence
        } else {
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);
            calculate_decayed_confidence(confidence, created_at, half_life_days)
        };

        // Filter out low-confidence relations
        if effective_confidence >= min_confidence {
            results.push(DecayedCrossRef {
                from_id,
                to_id,
                edge_type,
                score,
                original_confidence: confidence,
                effective_confidence,
                strength,
                pinned,
            });
        }
    }

    // Sort by effective score (score * confidence * strength)
    results.sort_by(|a, b| {
        let score_a = a.score * a.effective_confidence * a.strength;
        let score_b = b.score * b.effective_confidence * b.strength;
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(results)
}

/// Cross-reference with calculated decayed confidence
#[derive(Debug, Clone)]
pub struct DecayedCrossRef {
    pub from_id: MemoryId,
    pub to_id: MemoryId,
    pub edge_type: String,
    pub score: f32,
    pub original_confidence: f32,
    pub effective_confidence: f32,
    pub strength: f32,
    pub pinned: bool,
}

impl DecayedCrossRef {
    /// Calculate effective score considering all factors
    pub fn effective_score(&self) -> f32 {
        self.score * self.effective_confidence * self.strength
    }
}

/// Batch update confidence values (for maintenance)
pub fn refresh_confidence_batch(
    conn: &Connection,
    half_life_days: f32,
    min_confidence: f32,
) -> Result<RefreshResult> {
    let now = Utc::now();
    let now_str = now.to_rfc3339();

    // Get all non-pinned crossrefs
    let mut stmt = conn.prepare(
        "SELECT id, confidence, created_at FROM crossrefs
         WHERE pinned = 0 AND valid_to IS NULL",
    )?;

    let rows: Vec<(i64, f32, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let mut updated = 0;
    let mut expired = 0;

    for (id, original_confidence, created_at_str) in rows {
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or(now);

        let effective =
            calculate_decayed_confidence(original_confidence, created_at, half_life_days);

        if effective < min_confidence {
            // Mark as expired (soft delete)
            conn.execute(
                "UPDATE crossrefs SET valid_to = ? WHERE id = ?",
                params![now_str, id],
            )?;
            expired += 1;
        }
        updated += 1;
    }

    Ok(RefreshResult { updated, expired })
}

/// Result of batch confidence refresh
#[derive(Debug, Clone)]
pub struct RefreshResult {
    pub updated: i64,
    pub expired: i64,
}

/// Pin a cross-reference (exempt from decay)
pub fn pin_crossref(conn: &Connection, from_id: MemoryId, to_id: MemoryId) -> Result<()> {
    conn.execute(
        "UPDATE crossrefs SET pinned = 1 WHERE from_id = ? AND to_id = ? AND valid_to IS NULL",
        params![from_id, to_id],
    )?;
    Ok(())
}

/// Unpin a cross-reference (subject to decay)
pub fn unpin_crossref(conn: &Connection, from_id: MemoryId, to_id: MemoryId) -> Result<()> {
    conn.execute(
        "UPDATE crossrefs SET pinned = 0 WHERE from_id = ? AND to_id = ? AND valid_to IS NULL",
        params![from_id, to_id],
    )?;
    Ok(())
}

/// Boost confidence of a cross-reference (user interaction)
pub fn boost_confidence(
    conn: &Connection,
    from_id: MemoryId,
    to_id: MemoryId,
    boost: f32,
) -> Result<()> {
    // Boost is additive, capped at 1.0
    conn.execute(
        "UPDATE crossrefs SET confidence = MIN(1.0, confidence + ?)
         WHERE from_id = ? AND to_id = ? AND valid_to IS NULL",
        params![boost, from_id, to_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_decay() {
        // At half-life, confidence should be 50%
        let now = Utc::now();
        let half_life_ago = now - chrono::Duration::days(30);

        let decayed = calculate_decayed_confidence(1.0, half_life_ago, 30.0);
        assert!(
            (decayed - 0.5).abs() < 0.01,
            "Expected ~0.5, got {}",
            decayed
        );
    }

    #[test]
    fn test_confidence_decay_double_half_life() {
        // At 2x half-life, confidence should be 25%
        let now = Utc::now();
        let two_half_lives_ago = now - chrono::Duration::days(60);

        let decayed = calculate_decayed_confidence(1.0, two_half_lives_ago, 30.0);
        assert!(
            (decayed - 0.25).abs() < 0.01,
            "Expected ~0.25, got {}",
            decayed
        );
    }

    #[test]
    fn test_confidence_no_decay_for_new() {
        let now = Utc::now();
        let decayed = calculate_decayed_confidence(1.0, now, 30.0);
        assert!(
            (decayed - 1.0).abs() < 0.01,
            "Expected ~1.0, got {}",
            decayed
        );
    }
}
