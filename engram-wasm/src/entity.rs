//! Regex-based Named Entity Recognition — extracted from engram-core.
//!
//! Provides lightweight NER for:
//! - **@mentions**: `@alice`, `@acme-corp`
//! - **Email addresses**: `user@example.com`
//! - **URLs** (domain extraction): `https://example.com`
//! - **Capitalized names**: `John Smith`, `Alice Wonderland`
//!
//! This is a pure-computation extraction that does **not** require a database
//! connection. The alias-resolution step (looking up existing identities in
//! SQLite) from `entity_extraction.rs` is intentionally excluded.
//!
//! ## Invariants
//!
//! - Extraction never panics on any input.
//! - Empty/whitespace input returns empty results.
//! - Duplicate mentions are deduplicated (count field incremented).
//! - Results are sorted by first occurrence position.
//! - At most `max_entities` entities are returned.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Maximum entities to extract from a single text (prevents DoS).
const DEFAULT_MAX_ENTITIES: usize = 100;

/// Minimum confidence to include an entity in results.
const MIN_CONFIDENCE: f32 = 0.3;

// === Compiled regex patterns (compiled once at startup) ========================

static MENTION_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"@([a-zA-Z][a-zA-Z0-9_-]{1,30})").expect("valid regex"));

static EMAIL_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").expect("valid regex")
});

static URL_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"https?://([a-zA-Z0-9.-]+)(?:/[^\s]*)?").expect("valid regex"));

/// Two or more consecutive capitalized words (e.g. "John Smith").
static NAME_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b").expect("valid regex"));

// ==============================================================================

/// Type of extracted entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    /// `@mention` style reference
    Mention,
    /// Email address
    Email,
    /// URL domain
    Url,
    /// Capitalized name pattern
    Name,
}

impl EntityType {
    fn default_confidence(self) -> f32 {
        match self {
            EntityType::Mention => 0.9,
            EntityType::Email => 0.95,
            EntityType::Url => 0.7,
            EntityType::Name => 0.5,
        }
    }
}

/// A single extracted entity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Raw text as found in content.
    pub text: String,
    /// Normalized form (lowercase, trimmed).
    pub normalized: String,
    /// Entity type.
    pub entity_type: EntityType,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f32,
    /// Byte offset of first occurrence.
    pub position: usize,
    /// Number of occurrences in text.
    pub count: usize,
}

/// Configuration for entity extraction.
#[derive(Debug, Clone)]
pub struct ExtractConfig {
    pub extract_mentions: bool,
    pub extract_emails: bool,
    pub extract_urls: bool,
    pub extract_names: bool,
    pub min_confidence: f32,
    pub max_entities: usize,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            extract_mentions: true,
            extract_emails: true,
            extract_urls: true,
            extract_names: true,
            min_confidence: MIN_CONFIDENCE,
            max_entities: DEFAULT_MAX_ENTITIES,
        }
    }
}

/// Extract entities from text.
///
/// Never panics. Returns empty results for empty/whitespace input.
///
/// # Arguments
///
/// * `text`   — Text to extract entities from.
/// * `config` — Extraction configuration.
///
/// # Returns
///
/// Vec of entities, deduplicated and sorted by first occurrence position.
pub fn extract_entities(text: &str, config: &ExtractConfig) -> Vec<Entity> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    // HashMap from normalized key → entity for deduplication
    let mut map: HashMap<String, Entity> = HashMap::new();

    // @mentions
    if config.extract_mentions {
        for cap in MENTION_PATTERN.captures_iter(text) {
            if map.len() >= config.max_entities {
                break;
            }
            if let Some(m) = cap.get(1) {
                let raw = format!("@{}", m.as_str());
                let key = normalize(&raw);
                let pos = cap.get(0).map_or(0, |c: regex::Match| c.start());
                add_or_count(&mut map, raw, key, EntityType::Mention, pos);
            }
        }
    }

    // Email addresses
    if config.extract_emails && map.len() < config.max_entities {
        for m in EMAIL_PATTERN.find_iter(text) {
            if map.len() >= config.max_entities {
                break;
            }
            let raw: String = m.as_str().to_string();
            let key = normalize(&raw);
            add_or_count(&mut map, raw, key, EntityType::Email, m.start());
        }
    }

    // URLs — extract domain
    if config.extract_urls && map.len() < config.max_entities {
        for cap in URL_PATTERN.captures_iter(text) {
            if map.len() >= config.max_entities {
                break;
            }
            if let Some(domain) = cap.get(1) {
                let raw: String = domain.as_str().to_string();
                if !is_common_domain(&raw) {
                    let key = normalize(&raw);
                    let pos = cap.get(0).map_or(0, |c: regex::Match| c.start());
                    add_or_count(&mut map, raw, key, EntityType::Url, pos);
                }
            }
        }
    }

    // Capitalized names
    if config.extract_names && map.len() < config.max_entities {
        for m in NAME_PATTERN.find_iter(text) {
            if map.len() >= config.max_entities {
                break;
            }
            let raw: String = m.as_str().to_string();
            if !is_common_phrase(&raw) {
                let key = normalize(&raw);
                add_or_count(&mut map, raw, key, EntityType::Name, m.start());
            }
        }
    }

    // Filter by confidence, sort by position
    let mut entities: Vec<Entity> = map
        .into_values()
        .filter(|e| e.confidence >= config.min_confidence)
        .collect();
    entities.sort_by_key(|e| e.position);
    entities.truncate(config.max_entities);
    entities
}

// ==============================================================================
// Helpers
// ==============================================================================

/// Add entity or increment count if already seen.
fn add_or_count(
    map: &mut HashMap<String, Entity>,
    text: String,
    key: String,
    entity_type: EntityType,
    position: usize,
) {
    if let Some(existing) = map.get_mut(&key) {
        existing.count += 1;
    } else {
        map.insert(
            key.clone(),
            Entity {
                text,
                normalized: key,
                entity_type,
                confidence: entity_type.default_confidence(),
                position,
                count: 1,
            },
        );
    }
}

/// Normalize an entity text: lowercase + trim + strip leading `@`.
fn normalize(text: &str) -> String {
    text.trim().trim_start_matches('@').to_lowercase()
}

/// Skip overly-common domains that add no signal.
fn is_common_domain(domain: &str) -> bool {
    const COMMON: &[&str] = &[
        "google.com",
        "github.com",
        "stackoverflow.com",
        "wikipedia.org",
        "twitter.com",
        "x.com",
        "facebook.com",
        "youtube.com",
        "linkedin.com",
        "medium.com",
        "docs.rs",
        "crates.io",
        "rust-lang.org",
    ];
    COMMON.iter().any(|c| domain.eq_ignore_ascii_case(c))
}

/// Skip common phrases that are not meaningful names.
fn is_common_phrase(phrase: &str) -> bool {
    const COMMON: &[&str] = &[
        "New York",
        "Los Angeles",
        "San Francisco",
        "United States",
        "Open Source",
        "Machine Learning",
        "Artificial Intelligence",
        "The End",
        "The Start",
    ];
    COMMON.iter().any(|c| phrase.eq_ignore_ascii_case(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mentions() {
        let config = ExtractConfig::default();
        let entities = extract_entities("Hello @alice and @bob-smith!", &config);
        let mentions: Vec<&str> = entities.iter().map(|e| e.text.as_str()).collect();
        assert!(mentions.contains(&"@alice"));
        assert!(mentions.contains(&"@bob-smith"));
    }

    #[test]
    fn test_extract_email() {
        let config = ExtractConfig {
            extract_mentions: false,
            extract_names: false,
            extract_urls: false,
            ..Default::default()
        };
        let entities = extract_entities("contact: hello@example.com", &config);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::Email);
        assert_eq!(entities[0].text, "hello@example.com");
    }

    #[test]
    fn test_extract_url_domain() {
        let config = ExtractConfig {
            extract_mentions: false,
            extract_names: false,
            extract_emails: false,
            ..Default::default()
        };
        let entities = extract_entities("Visit https://mycompany.io/docs for details", &config);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::Url);
        assert_eq!(entities[0].text, "mycompany.io");
    }

    #[test]
    fn test_extract_name() {
        let config = ExtractConfig {
            extract_mentions: false,
            extract_emails: false,
            extract_urls: false,
            ..Default::default()
        };
        let entities = extract_entities("I met John Smith yesterday.", &config);
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_type, EntityType::Name);
        assert_eq!(entities[0].text, "John Smith");
    }

    #[test]
    fn test_deduplication() {
        let config = ExtractConfig::default();
        let entities = extract_entities("@alice waved. @alice smiled.", &config);
        let alice: Vec<&Entity> = entities.iter().filter(|e| e.text == "@alice").collect();
        assert_eq!(alice.len(), 1, "Should deduplicate @alice");
        assert_eq!(alice[0].count, 2, "Count should be 2");
    }

    #[test]
    fn test_empty_input() {
        let config = ExtractConfig::default();
        assert!(extract_entities("", &config).is_empty());
        assert!(extract_entities("   ", &config).is_empty());
    }

    #[test]
    fn test_max_entities_bound() {
        let config = ExtractConfig {
            max_entities: 2,
            ..Default::default()
        };
        let entities = extract_entities("@a @b @c @d @e @f", &config);
        assert!(entities.len() <= 2);
    }

    #[test]
    fn test_common_domain_skipped() {
        let config = ExtractConfig {
            extract_mentions: false,
            extract_names: false,
            extract_emails: false,
            ..Default::default()
        };
        let entities = extract_entities("See https://github.com/foo/bar", &config);
        assert!(entities.is_empty(), "github.com should be filtered as common domain");
    }

    #[test]
    fn test_never_panics_edge_cases() {
        let config = ExtractConfig::default();
        let cases = [
            "",
            "   ",
            "@",
            "@@@@",
            "@a",
            "http://",
            "https://",
            "emoji 🎉🎊",
            "unicode 日本語",
            "\0\0\0",
        ];
        for &input in &cases {
            let _ = extract_entities(input, &config); // must not panic
        }
    }
}
