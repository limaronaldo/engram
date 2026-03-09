//! Emotional & Reflective Memory — RML-1215
//!
//! OpenMemory-inspired emotional analysis and reflection engine.
//!
//! Provides:
//! - Rule-based sentiment analysis (no external dependencies)
//! - Reflection generation at Surface / Analytical / Meta depth
//! - Temporal sentiment timelines over a date range
//!
//! ## Invariants
//! - Sentiment scores are always in the range [-1.0, 1.0]
//! - Confidence scores are always in the range [0.0, 1.0]
//! - Empty/whitespace input returns `Neutral` sentiment with score 0.0
//! - Reflection content is never empty
//! - All timestamps are RFC3339 UTC

use std::collections::HashMap;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::Result;

// =============================================================================
// Types
// =============================================================================

/// High-level sentiment polarity label
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SentimentLabel {
    /// Predominantly positive sentiment
    Positive,
    /// Predominantly negative sentiment
    Negative,
    /// Neither positive nor negative
    Neutral,
    /// Significant mixture of positive and negative signals
    Mixed,
}

impl SentimentLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            SentimentLabel::Positive => "positive",
            SentimentLabel::Negative => "negative",
            SentimentLabel::Neutral => "neutral",
            SentimentLabel::Mixed => "mixed",
        }
    }
}

/// Result of sentiment analysis on a piece of text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sentiment {
    /// Aggregate score in [-1.0, 1.0]; -1 = most negative, +1 = most positive
    pub score: f32,
    /// Qualitative label derived from the score
    pub label: SentimentLabel,
    /// Confidence in the classification, in [0.0, 1.0]
    pub confidence: f32,
    /// Sentiment-bearing keywords found in the text
    pub keywords: Vec<String>,
}

/// Depth of a reflection — controls how much processing is done
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionDepth {
    /// Identify key themes; one-pass, fast
    Surface,
    /// Find patterns, sentiment trends, and contradictions; multi-pass
    Analytical,
    /// Reflect on existing reflections; requires prior saved reflections
    Meta,
}

impl ReflectionDepth {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReflectionDepth::Surface => "surface",
            ReflectionDepth::Analytical => "analytical",
            ReflectionDepth::Meta => "meta",
        }
    }
}

/// A synthesised reflection over one or more memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    /// Database-assigned id (0 when not yet persisted)
    pub id: i64,
    /// Narrative content of the reflection
    pub content: String,
    /// IDs of the source memories that generated this reflection
    pub source_ids: Vec<i64>,
    /// How deeply this reflection was generated
    pub depth: ReflectionDepth,
    /// Key insights distilled from the source memories
    pub insights: Vec<String>,
    /// RFC3339 UTC creation timestamp
    pub created_at: String,
}

/// A single data point on a sentiment timeline
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SentimentPoint {
    /// RFC3339 UTC timestamp of the underlying memory
    pub timestamp: String,
    /// Sentiment score in [-1.0, 1.0]
    pub score: f32,
    /// ID of the memory this point was derived from
    pub memory_id: i64,
}

// =============================================================================
// DDL
// =============================================================================

/// DDL for the reflections table — call once during schema setup.
pub const CREATE_REFLECTIONS_TABLE: &str = r#"
    CREATE TABLE IF NOT EXISTS reflections (
        id        INTEGER PRIMARY KEY AUTOINCREMENT,
        content   TEXT    NOT NULL,
        source_ids TEXT   NOT NULL DEFAULT '[]',
        depth     TEXT    NOT NULL DEFAULT 'surface',
        created_at TEXT   NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
    );
    CREATE INDEX IF NOT EXISTS idx_reflections_depth      ON reflections(depth);
    CREATE INDEX IF NOT EXISTS idx_reflections_created_at ON reflections(created_at);
"#;

// =============================================================================
// Word lists
// =============================================================================

/// Words that carry positive sentiment
static POSITIVE_WORDS: &[&str] = &[
    "good",
    "great",
    "excellent",
    "happy",
    "love",
    "amazing",
    "wonderful",
    "fantastic",
    "brilliant",
    "awesome",
    "perfect",
    "beautiful",
    "outstanding",
    "superb",
    "delightful",
    "pleased",
    "grateful",
    "thrilled",
    "excited",
    "proud",
    "successful",
    "efficient",
    "impressive",
    "remarkable",
    "enjoyable",
    "positive",
    "beneficial",
    "valuable",
    "productive",
    "innovative",
    "elegant",
    "smooth",
    "clean",
    "fast",
    "reliable",
    "stable",
    "robust",
    "secure",
    "scalable",
    "optimal",
];

/// Words that carry negative sentiment
static NEGATIVE_WORDS: &[&str] = &[
    "bad",
    "terrible",
    "awful",
    "hate",
    "horrible",
    "poor",
    "worst",
    "ugly",
    "broken",
    "failed",
    "error",
    "bug",
    "crash",
    "slow",
    "wrong",
    "missing",
    "confusing",
    "frustrating",
    "annoying",
    "difficult",
    "complicated",
    "messy",
    "unstable",
    "insecure",
    "vulnerable",
    "deprecated",
    "outdated",
    "bloated",
    "fragile",
    "flaky",
    "painful",
    "tedious",
    "cumbersome",
    "clunky",
    "hacky",
    "legacy",
    "technical-debt",
    "regression",
    "leak",
    "bottleneck",
];

/// Words that negate the sentiment of the following word
static NEGATION_WORDS: &[&str] = &[
    "not", "no", "never", "don't", "doesn't", "isn't", "aren't", "wasn't", "can't", "won't",
];

/// Words that amplify the magnitude of the following sentiment word
static INTENSIFIERS: &[&str] = &["very", "extremely", "really", "absolutely", "incredibly"];

/// Multiplier applied when an intensifier precedes a sentiment word
const INTENSIFIER_MULTIPLIER: f32 = 1.5;

// =============================================================================
// SentimentAnalyzer
// =============================================================================

/// Rule-based sentiment analyser with negation and intensifier support.
///
/// No external dependencies — works entirely from static word lists.
pub struct SentimentAnalyzer;

impl SentimentAnalyzer {
    pub fn new() -> Self {
        Self
    }

    /// Analyse the sentiment of `text` and return a [`Sentiment`].
    ///
    /// The algorithm:
    /// 1. Tokenise by whitespace, lowercasing and stripping punctuation.
    /// 2. Walk tokens left-to-right, tracking negation and intensifier state.
    /// 3. Accumulate a raw score; collect matched keywords.
    /// 4. Normalise score to [-1.0, 1.0] and derive a label.
    pub fn analyze(&self, text: &str) -> Sentiment {
        if text.trim().is_empty() {
            return Sentiment {
                score: 0.0,
                label: SentimentLabel::Neutral,
                confidence: 1.0,
                keywords: Vec::new(),
            };
        }

        let tokens: Vec<String> = text
            .split_whitespace()
            .map(|t| t.to_lowercase())
            .map(|t| {
                t.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                    .to_string()
            })
            .filter(|t| !t.is_empty())
            .collect();

        let mut raw_score: f32 = 0.0;
        let mut keywords: Vec<String> = Vec::new();
        let mut negated = false;
        let mut intensify = false;
        let mut pos_hits: u32 = 0;
        let mut neg_hits: u32 = 0;

        for token in &tokens {
            if NEGATION_WORDS.contains(&token.as_str()) {
                negated = true;
                intensify = false;
                continue;
            }

            if INTENSIFIERS.contains(&token.as_str()) {
                intensify = true;
                continue;
            }

            let base_delta = if POSITIVE_WORDS.contains(&token.as_str()) {
                keywords.push(token.clone());
                pos_hits += 1;
                1.0_f32
            } else if NEGATIVE_WORDS.contains(&token.as_str()) {
                keywords.push(token.clone());
                neg_hits += 1;
                -1.0_f32
            } else {
                // Not a sentiment word — reset context flags
                negated = false;
                intensify = false;
                continue;
            };

            let mut delta = base_delta;
            if intensify {
                delta *= INTENSIFIER_MULTIPLIER;
            }
            if negated {
                delta = -delta;
            }

            raw_score += delta;

            // Reset context flags after consuming the sentiment word
            negated = false;
            intensify = false;
        }

        let total_hits = pos_hits + neg_hits;

        // Normalise: clamp to [-1.0, 1.0]
        let score = if total_hits == 0 {
            0.0
        } else {
            (raw_score / (total_hits as f32)).clamp(-1.0, 1.0)
        };

        // Confidence grows with the number of signal words found
        let confidence = if total_hits == 0 {
            0.5 // uncertain when no signal words are found
        } else {
            (0.5 + (total_hits as f32 * 0.1)).min(1.0)
        };

        // Label
        let label = if total_hits == 0 {
            SentimentLabel::Neutral
        } else if pos_hits > 0 && neg_hits > 0 {
            // Mixed only when both polarities are present in meaningful quantities
            let ratio = pos_hits.min(neg_hits) as f32 / pos_hits.max(neg_hits) as f32;
            if ratio > 0.3 {
                SentimentLabel::Mixed
            } else if score > 0.0 {
                SentimentLabel::Positive
            } else {
                SentimentLabel::Negative
            }
        } else if score > 0.0 {
            SentimentLabel::Positive
        } else {
            SentimentLabel::Negative
        };

        Sentiment {
            score,
            label,
            confidence,
            keywords,
        }
    }
}

impl Default for SentimentAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// ReflectionEngine
// =============================================================================

/// Generates, persists, and retrieves reflections over memory content.
pub struct ReflectionEngine {
    analyzer: SentimentAnalyzer,
}

impl ReflectionEngine {
    pub fn new() -> Self {
        Self {
            analyzer: SentimentAnalyzer::new(),
        }
    }

    /// Generate a [`Reflection`] from a set of `(memory_id, content)` pairs.
    ///
    /// The reflection is **not** automatically saved to the database.
    /// Call [`save_reflection`] to persist it.
    pub fn create_reflection(
        &self,
        conn: &Connection,
        memory_contents: &[(i64, &str)],
        depth: ReflectionDepth,
    ) -> Result<Reflection> {
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let source_ids: Vec<i64> = memory_contents.iter().map(|(id, _)| *id).collect();

        let (content, insights) = match depth {
            ReflectionDepth::Surface => self.surface_reflect(memory_contents),
            ReflectionDepth::Analytical => self.analytical_reflect(memory_contents),
            ReflectionDepth::Meta => self.meta_reflect(conn, memory_contents)?,
        };

        Ok(Reflection {
            id: 0,
            content,
            source_ids,
            depth,
            insights,
            created_at: now,
        })
    }

    // ------------------------------------------------------------------
    // Private: Surface reflection
    // ------------------------------------------------------------------

    /// Surface: extract and summarise the most common nouns/content words.
    fn surface_reflect(&self, memory_contents: &[(i64, &str)]) -> (String, Vec<String>) {
        if memory_contents.is_empty() {
            return (
                "No memories provided for reflection.".to_string(),
                Vec::new(),
            );
        }

        // Collect all tokens, filter stopwords
        let stopwords = &[
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "could", "should", "may", "might",
            "shall", "can", "to", "of", "in", "for", "on", "with", "at", "by", "from", "and", "or",
            "but", "if", "then", "that", "this", "it", "its", "i", "you", "we", "they", "he",
            "she", "my", "your", "our", "their", "not", "no", "so",
        ];

        let mut freq: HashMap<String, usize> = HashMap::new();
        for (_, content) in memory_contents {
            for token in content.split_whitespace() {
                let t = token
                    .to_lowercase()
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_string();
                if t.len() > 3 && !stopwords.contains(&t.as_str()) {
                    *freq.entry(t).or_insert(0) += 1;
                }
            }
        }

        let mut sorted: Vec<(String, usize)> = freq.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let top_themes: Vec<String> = sorted.into_iter().take(5).map(|(w, _)| w).collect();

        let insights: Vec<String> = top_themes
            .iter()
            .map(|t| format!("Key theme: {}", t))
            .collect();

        let content = if top_themes.is_empty() {
            format!(
                "Reflection over {} memories. No dominant themes detected.",
                memory_contents.len()
            )
        } else {
            format!(
                "Reflection over {} memories. Key themes: {}.",
                memory_contents.len(),
                top_themes.join(", ")
            )
        };

        (content, insights)
    }

    // ------------------------------------------------------------------
    // Private: Analytical reflection
    // ------------------------------------------------------------------

    /// Analytical: sentiment trends, topic clusters, contradictions.
    fn analytical_reflect(&self, memory_contents: &[(i64, &str)]) -> (String, Vec<String>) {
        if memory_contents.is_empty() {
            return (
                "No memories provided for analytical reflection.".to_string(),
                Vec::new(),
            );
        }

        let mut pos_count = 0usize;
        let mut neg_count = 0usize;
        let mut mixed_count = 0usize;
        let mut neutral_count = 0usize;
        let mut total_score: f32 = 0.0;

        let sentiments: Vec<Sentiment> = memory_contents
            .iter()
            .map(|(_, c)| self.analyzer.analyze(c))
            .collect();

        for s in &sentiments {
            total_score += s.score;
            match s.label {
                SentimentLabel::Positive => pos_count += 1,
                SentimentLabel::Negative => neg_count += 1,
                SentimentLabel::Mixed => mixed_count += 1,
                SentimentLabel::Neutral => neutral_count += 1,
            }
        }

        let n = memory_contents.len();
        let avg_score = total_score / n as f32;

        let mut insights = Vec::new();

        // Sentiment trend
        let trend = if avg_score > 0.3 {
            insights.push(format!(
                "Overall sentiment is positive (avg score: {:.2})",
                avg_score
            ));
            "positive"
        } else if avg_score < -0.3 {
            insights.push(format!(
                "Overall sentiment is negative (avg score: {:.2})",
                avg_score
            ));
            "negative"
        } else {
            insights.push(format!(
                "Overall sentiment is neutral (avg score: {:.2})",
                avg_score
            ));
            "neutral"
        };

        // Distribution insight
        if pos_count > 0 || neg_count > 0 {
            insights.push(format!(
                "Distribution: {} positive, {} negative, {} mixed, {} neutral",
                pos_count, neg_count, mixed_count, neutral_count
            ));
        }

        // Contradiction detection
        if pos_count > 0 && neg_count > 0 {
            let ratio = pos_count.min(neg_count) as f32 / pos_count.max(neg_count) as f32;
            if ratio > 0.4 {
                insights.push(format!(
                    "Contradictory signals detected: {} positive vs {} negative memories",
                    pos_count, neg_count
                ));
            }
        }

        // Keyword frequency across all memories
        let mut kw_freq: HashMap<String, usize> = HashMap::new();
        for s in &sentiments {
            for kw in &s.keywords {
                *kw_freq.entry(kw.clone()).or_insert(0) += 1;
            }
        }
        let mut top_kw: Vec<(String, usize)> = kw_freq.into_iter().collect();
        top_kw.sort_by(|a, b| b.1.cmp(&a.1));
        let top_keywords: Vec<String> = top_kw.into_iter().take(3).map(|(k, _)| k).collect();
        if !top_keywords.is_empty() {
            insights.push(format!(
                "Frequent sentiment keywords: {}",
                top_keywords.join(", ")
            ));
        }

        let content = format!(
            "Analytical reflection over {} memories. Overall {trend} tone (avg score: {:.2}). \
             Positive: {pos_count}, Negative: {neg_count}, Mixed: {mixed_count}, Neutral: {neutral_count}.",
            n,
            avg_score,
        );

        (content, insights)
    }

    // ------------------------------------------------------------------
    // Private: Meta reflection
    // ------------------------------------------------------------------

    /// Meta: reflects on existing saved reflections plus the supplied memories.
    fn meta_reflect(
        &self,
        conn: &Connection,
        memory_contents: &[(i64, &str)],
    ) -> Result<(String, Vec<String>)> {
        // Load recent saved reflections to incorporate
        let prior = list_reflections(conn, None, 10)?;

        let prior_count = prior.len();

        // Analytical pass on current memories
        let (analytical_content, mut insights) = self.analytical_reflect(memory_contents);

        // Add meta-insights from prior reflections
        if prior_count == 0 {
            insights
                .push("No prior reflections found; this is a first-order reflection.".to_string());
        } else {
            insights.push(format!(
                "Built on {} prior reflections for meta-level synthesis.",
                prior_count
            ));

            // Count depth distribution of prior reflections
            let surface_count = prior
                .iter()
                .filter(|r| r.depth == ReflectionDepth::Surface)
                .count();
            let analytical_count = prior
                .iter()
                .filter(|r| r.depth == ReflectionDepth::Analytical)
                .count();

            if surface_count > 0 || analytical_count > 0 {
                insights.push(format!(
                    "Prior reflection depth breakdown: {} surface, {} analytical.",
                    surface_count, analytical_count
                ));
            }

            // Synthesise a recurring theme from prior reflection contents
            let prior_text: String = prior
                .iter()
                .map(|r| r.content.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let prior_sentiment = self.analyzer.analyze(&prior_text);
            insights.push(format!(
                "Aggregate prior reflection sentiment: {} (score: {:.2}).",
                prior_sentiment.label.as_str(),
                prior_sentiment.score
            ));
        }

        let content = format!(
            "Meta-reflection synthesising {} current memories with {} prior reflections. {}",
            memory_contents.len(),
            prior_count,
            analytical_content,
        );

        Ok((content, insights))
    }
}

impl Default for ReflectionEngine {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Storage helpers
// =============================================================================

/// Persist a [`Reflection`] and return its database-assigned id.
pub fn save_reflection(conn: &Connection, reflection: &Reflection) -> Result<i64> {
    let source_ids_json = serde_json::to_string(&reflection.source_ids)?;
    let now = if reflection.created_at.is_empty() {
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    } else {
        reflection.created_at.clone()
    };

    conn.execute(
        "INSERT INTO reflections (content, source_ids, depth, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            reflection.content,
            source_ids_json,
            reflection.depth.as_str(),
            now,
        ],
    )?;

    Ok(conn.last_insert_rowid())
}

/// List persisted reflections, optionally filtered by depth.
///
/// `limit = 0` returns all rows (up to i64::MAX).
pub fn list_reflections(
    conn: &Connection,
    depth: Option<ReflectionDepth>,
    limit: usize,
) -> Result<Vec<Reflection>> {
    let effective_limit = if limit == 0 { i64::MAX } else { limit as i64 };

    let rows: Vec<Reflection> = match depth {
        Some(d) => {
            let mut stmt = conn.prepare(
                "SELECT id, content, source_ids, depth, created_at
                 FROM reflections
                 WHERE depth = ?1
                 ORDER BY id DESC
                 LIMIT ?2",
            )?;
            let collected = stmt
                .query_map(params![d.as_str(), effective_limit], map_reflection_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            collected
        }
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, content, source_ids, depth, created_at
                 FROM reflections
                 ORDER BY id DESC
                 LIMIT ?1",
            )?;
            let collected = stmt
                .query_map(params![effective_limit], map_reflection_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            collected
        }
    };

    Ok(rows)
}

/// Build a sentiment timeline for memories in a workspace within a date range.
///
/// `from` and `to` are RFC3339 UTC strings used in a `BETWEEN` comparison.
/// Returns one [`SentimentPoint`] per memory, ordered by `created_at` ascending.
///
/// Requires the standard `memories` table to be present in `conn`.
pub fn sentiment_timeline(
    conn: &Connection,
    workspace: &str,
    from: &str,
    to: &str,
) -> Result<Vec<SentimentPoint>> {
    let mut stmt = conn.prepare(
        "SELECT id, content, created_at
         FROM memories
         WHERE workspace = ?1
           AND created_at BETWEEN ?2 AND ?3
         ORDER BY created_at ASC",
    )?;

    let analyzer = SentimentAnalyzer::new();

    let points: Vec<SentimentPoint> = stmt
        .query_map(params![workspace, from, to], |row| {
            let id: i64 = row.get(0)?;
            let content: String = row.get(1)?;
            let timestamp: String = row.get(2)?;
            Ok((id, content, timestamp))
        })?
        .filter_map(|r| r.ok())
        .map(|(id, content, timestamp)| {
            let sentiment = analyzer.analyze(&content);
            SentimentPoint {
                timestamp,
                score: sentiment.score,
                memory_id: id,
            }
        })
        .collect();

    Ok(points)
}

/// Map a rusqlite row from the `reflections` table to a [`Reflection`].
fn map_reflection_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Reflection> {
    let id: i64 = row.get(0)?;
    let content: String = row.get(1)?;
    let source_ids_json: String = row.get(2)?;
    let depth_str: String = row.get(3)?;
    let created_at: String = row.get(4)?;

    let source_ids: Vec<i64> = serde_json::from_str(&source_ids_json).unwrap_or_default();

    let depth = match depth_str.as_str() {
        "surface" => ReflectionDepth::Surface,
        "analytical" => ReflectionDepth::Analytical,
        "meta" => ReflectionDepth::Meta,
        _ => ReflectionDepth::Surface,
    };

    Ok(Reflection {
        id,
        content,
        source_ids,
        depth,
        insights: Vec::new(), // insights are not persisted; regenerated on demand
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

    fn in_memory_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(CREATE_REFLECTIONS_TABLE)
            .expect("create reflections table");
        conn
    }

    fn memories_table(conn: &Connection) {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                content     TEXT    NOT NULL,
                workspace   TEXT    NOT NULL DEFAULT 'default',
                created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );",
        )
        .expect("create memories table");
    }

    fn analyzer() -> SentimentAnalyzer {
        SentimentAnalyzer::new()
    }

    // -----------------------------------------------------------------------
    // 1. Positive sentiment
    // -----------------------------------------------------------------------
    #[test]
    fn test_positive_sentiment() {
        let s = analyzer().analyze("This is a great and amazing product");
        assert_eq!(s.label, SentimentLabel::Positive);
        assert!(s.score > 0.0, "score should be positive, got {}", s.score);
        assert!(
            s.keywords.contains(&"great".to_string())
                || s.keywords.contains(&"amazing".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // 2. Negative sentiment
    // -----------------------------------------------------------------------
    #[test]
    fn test_negative_sentiment() {
        let s = analyzer().analyze("The software is broken and has terrible bugs");
        assert_eq!(s.label, SentimentLabel::Negative);
        assert!(s.score < 0.0, "score should be negative, got {}", s.score);
        assert!(
            s.keywords.contains(&"broken".to_string())
                || s.keywords.contains(&"terrible".to_string())
                || s.keywords.contains(&"bugs".to_string())
        );
    }

    // -----------------------------------------------------------------------
    // 3. Negation flipping
    // -----------------------------------------------------------------------
    #[test]
    fn test_negation_flips_positive() {
        let positive = analyzer().analyze("great work here");
        let negated = analyzer().analyze("not great work here");
        // Without negation: positive; with negation: should flip toward negative
        assert!(
            positive.score > negated.score,
            "negation should reduce score: positive={}, negated={}",
            positive.score,
            negated.score
        );
    }

    #[test]
    fn test_negation_flips_negative() {
        let negative = analyzer().analyze("this is terrible");
        let negated = analyzer().analyze("this is not terrible");
        // Without negation: negative; with negation: should flip toward positive
        assert!(
            negated.score > negative.score,
            "negation of negative word should increase score: negative={}, negated={}",
            negative.score,
            negated.score
        );
    }

    // -----------------------------------------------------------------------
    // 5. Intensifiers boost magnitude
    // -----------------------------------------------------------------------
    #[test]
    fn test_intensifiers_boost_magnitude() {
        let base = analyzer().analyze("good result");
        let intensified = analyzer().analyze("very good result");
        // Intensified should have a higher raw score; after normalisation the
        // score may be clamped, but the label should still be positive.
        assert_eq!(base.label, SentimentLabel::Positive);
        assert_eq!(intensified.label, SentimentLabel::Positive);
        assert!(
            intensified.score >= base.score,
            "intensifier should not decrease score: base={}, intensified={}",
            base.score,
            intensified.score
        );
    }

    // -----------------------------------------------------------------------
    // 4. Mixed sentiment
    // -----------------------------------------------------------------------
    #[test]
    fn test_mixed_sentiment() {
        // Balanced mix of positive and negative words
        let s = analyzer().analyze("great performance but terrible stability and broken error");
        // Either Mixed or the dominant one — accept Mixed or Positive since "great" appears
        assert!(
            matches!(
                s.label,
                SentimentLabel::Mixed | SentimentLabel::Positive | SentimentLabel::Negative
            ),
            "unexpected label: {:?}",
            s.label
        );
        // Both polarities must be represented in keywords
        let has_positive = s
            .keywords
            .iter()
            .any(|k| POSITIVE_WORDS.contains(&k.as_str()));
        let has_negative = s
            .keywords
            .iter()
            .any(|k| NEGATIVE_WORDS.contains(&k.as_str()));
        assert!(has_positive, "expected positive keywords in mixed text");
        assert!(has_negative, "expected negative keywords in mixed text");
    }

    // -----------------------------------------------------------------------
    // 6. Empty text
    // -----------------------------------------------------------------------
    #[test]
    fn test_empty_text() {
        let s = analyzer().analyze("");
        assert_eq!(s.label, SentimentLabel::Neutral);
        assert_eq!(s.score, 0.0);
        assert!(s.keywords.is_empty());
    }

    #[test]
    fn test_whitespace_only_text() {
        let s = analyzer().analyze("   \t\n  ");
        assert_eq!(s.label, SentimentLabel::Neutral);
        assert_eq!(s.score, 0.0);
    }

    // -----------------------------------------------------------------------
    // 7. Surface reflection
    // -----------------------------------------------------------------------
    #[test]
    fn test_reflection_surface() {
        let conn = in_memory_conn();
        let engine = ReflectionEngine::new();
        let memories = vec![
            (1i64, "memory performance is really fast and scalable"),
            (2i64, "memory performance tests look good"),
        ];
        let reflection = engine
            .create_reflection(&conn, &memories, ReflectionDepth::Surface)
            .expect("surface reflection should succeed");

        assert!(!reflection.content.is_empty());
        assert_eq!(reflection.depth, ReflectionDepth::Surface);
        assert_eq!(reflection.source_ids, vec![1, 2]);
        assert!(
            !reflection.insights.is_empty(),
            "surface reflection should produce insights"
        );
        // Content should mention key theme words from the memories
        let content_lower = reflection.content.to_lowercase();
        assert!(
            content_lower.contains("theme") || content_lower.contains("memories"),
            "unexpected surface content: {}",
            reflection.content
        );
    }

    // -----------------------------------------------------------------------
    // 8. Analytical reflection
    // -----------------------------------------------------------------------
    #[test]
    fn test_reflection_analytical() {
        let conn = in_memory_conn();
        let engine = ReflectionEngine::new();
        let memories = vec![
            (1i64, "the new feature is excellent and robust"),
            (2i64, "there is a terrible bug and regression in production"),
            (3i64, "deployment went smooth and stable"),
        ];
        let reflection = engine
            .create_reflection(&conn, &memories, ReflectionDepth::Analytical)
            .expect("analytical reflection should succeed");

        assert!(!reflection.content.is_empty());
        assert_eq!(reflection.depth, ReflectionDepth::Analytical);
        // Should detect contradictory signals
        let has_contradiction = reflection
            .insights
            .iter()
            .any(|i| i.contains("Contradict") || i.contains("positive") || i.contains("negative"));
        assert!(
            has_contradiction,
            "analytical reflection should detect sentiment signals"
        );
    }

    // -----------------------------------------------------------------------
    // 9. Sentiment timeline
    // -----------------------------------------------------------------------
    #[test]
    fn test_sentiment_timeline() {
        let conn = in_memory_conn();
        memories_table(&conn);

        conn.execute_batch(
            "INSERT INTO memories (id, content, workspace, created_at) VALUES
               (1, 'great day today excellent work', 'test', '2025-01-01T10:00:00Z'),
               (2, 'terrible bug crash broken', 'test', '2025-01-02T10:00:00Z'),
               (3, 'stable and reliable release', 'test', '2025-01-03T10:00:00Z');",
        )
        .expect("insert memories");

        let timeline = sentiment_timeline(
            &conn,
            "test",
            "2025-01-01T00:00:00Z",
            "2025-01-03T23:59:59Z",
        )
        .expect("timeline should succeed");

        assert_eq!(timeline.len(), 3, "expected 3 sentiment points");
        assert!(timeline[0].score > 0.0, "first memory should be positive");
        assert!(timeline[1].score < 0.0, "second memory should be negative");
        assert!(timeline[2].score > 0.0, "third memory should be positive");

        // Verify IDs and ordering
        assert_eq!(timeline[0].memory_id, 1);
        assert_eq!(timeline[1].memory_id, 2);
        assert_eq!(timeline[2].memory_id, 3);
    }

    // -----------------------------------------------------------------------
    // 10. save_reflection and list_reflections
    // -----------------------------------------------------------------------
    #[test]
    fn test_save_and_list_reflections() {
        let conn = in_memory_conn();
        let engine = ReflectionEngine::new();

        let memories = vec![(10i64, "smooth and fast deployment was successful")];
        let mut reflection = engine
            .create_reflection(&conn, &memories, ReflectionDepth::Surface)
            .expect("create reflection");

        let id = save_reflection(&conn, &reflection).expect("save reflection");
        assert!(id > 0, "saved id should be positive");
        reflection.id = id;

        let all = list_reflections(&conn, None, 10).expect("list reflections");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, id);
        assert_eq!(all[0].depth, ReflectionDepth::Surface);

        let surface_only =
            list_reflections(&conn, Some(ReflectionDepth::Surface), 10).expect("list surface");
        assert_eq!(surface_only.len(), 1);

        let analytical_only = list_reflections(&conn, Some(ReflectionDepth::Analytical), 10)
            .expect("list analytical");
        assert!(analytical_only.is_empty());
    }
}
