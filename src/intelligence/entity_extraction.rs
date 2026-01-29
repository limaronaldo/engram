//! Entity extraction for automatic identity linking
//!
//! Provides lightweight Named Entity Recognition (NER) for:
//! - @mentions (e.g., @ronaldo, @acme-corp)
//! - Email addresses
//! - URLs with domain extraction
//! - Capitalized names (simple heuristic)
//! - Known identity aliases (database lookup)
//!
//! ## Invariants
//!
//! - Extraction never panics on any input
//! - Empty/whitespace input returns empty results
//! - Duplicate mentions are deduplicated with count
//! - Results are sorted by first occurrence position
//!
//! ## Performance
//!
//! - Regex patterns are compiled once (lazy_static)
//! - Single pass through text for pattern matching
//! - Bounded output: max 100 entities per text

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument, warn};

use crate::error::Result;
use crate::storage::identity_links::{normalize_alias, resolve_alias};

/// Maximum entities to extract from a single text (prevents DoS)
const MAX_ENTITIES_PER_TEXT: usize = 100;

/// Minimum confidence threshold for extraction
const MIN_CONFIDENCE: f32 = 0.3;

/// Compiled regex patterns (compiled once, reused)
static MENTION_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"@([a-zA-Z][a-zA-Z0-9_-]{1,30})").expect("valid regex"));

static EMAIL_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").expect("valid regex")
});

static URL_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"https?://([a-zA-Z0-9.-]+)(?:/[^\s]*)?").expect("valid regex"));

/// Pattern for capitalized names (2+ words starting with capitals)
static NAME_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b").expect("valid regex"));

/// An extracted entity from text
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    /// The raw text as found in content
    pub mention_text: String,
    /// Normalized form for matching
    pub normalized: String,
    /// Type of entity detected
    pub entity_type: ExtractedEntityType,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Position in text (byte offset)
    pub position: usize,
    /// Number of times this entity appears
    pub count: usize,
    /// Resolved canonical ID if matched to existing identity
    pub resolved_id: Option<String>,
}

/// Type of extracted entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtractedEntityType {
    /// @mention style reference
    Mention,
    /// Email address
    Email,
    /// URL/domain
    Url,
    /// Capitalized name pattern
    Name,
    /// Matched existing alias
    KnownAlias,
}

impl ExtractedEntityType {
    /// Default confidence for this entity type
    fn default_confidence(&self) -> f32 {
        match self {
            ExtractedEntityType::Mention => 0.9,
            ExtractedEntityType::Email => 0.95,
            ExtractedEntityType::Url => 0.7,
            ExtractedEntityType::Name => 0.5,
            ExtractedEntityType::KnownAlias => 1.0,
        }
    }
}

/// Configuration for entity extraction
#[derive(Debug, Clone)]
pub struct ExtractionConfig {
    /// Extract @mentions
    pub extract_mentions: bool,
    /// Extract email addresses
    pub extract_emails: bool,
    /// Extract URLs/domains
    pub extract_urls: bool,
    /// Extract capitalized names
    pub extract_names: bool,
    /// Lookup existing aliases in database
    pub lookup_aliases: bool,
    /// Minimum confidence to include
    pub min_confidence: f32,
    /// Maximum entities to return
    pub max_entities: usize,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            extract_mentions: true,
            extract_emails: true,
            extract_urls: true,
            extract_names: true,
            lookup_aliases: true,
            min_confidence: MIN_CONFIDENCE,
            max_entities: MAX_ENTITIES_PER_TEXT,
        }
    }
}

/// Result of entity extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Extracted entities, deduplicated and sorted by position
    pub entities: Vec<ExtractedEntity>,
    /// Total mentions found (before dedup)
    pub total_mentions: usize,
    /// Number of entities resolved to existing identities
    pub resolved_count: usize,
}

/// Extract entities from text content.
///
/// This function never panics. Invalid input returns empty results.
///
/// # Arguments
/// * `content` - Text to extract entities from
/// * `config` - Extraction configuration
/// * `conn` - Optional database connection for alias lookup
///
/// # Returns
/// Extraction result with deduplicated, sorted entities
#[instrument(skip(content, config, conn), fields(content_len = content.len()))]
pub fn extract_entities(
    content: &str,
    config: &ExtractionConfig,
    conn: Option<&Connection>,
) -> ExtractionResult {
    // Handle empty/whitespace input
    let content = content.trim();
    if content.is_empty() {
        return ExtractionResult {
            entities: vec![],
            total_mentions: 0,
            resolved_count: 0,
        };
    }

    // Use HashMap for deduplication (normalized -> entity)
    let mut entities_map: HashMap<String, ExtractedEntity> = HashMap::new();
    let mut total_mentions = 0;

    // Extract @mentions
    if config.extract_mentions {
        for cap in MENTION_PATTERN.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                let mention_text = format!("@{}", m.as_str());
                let normalized = normalize_alias(&mention_text);
                let position = cap.get(0).map(|c| c.start()).unwrap_or(0);

                add_or_increment(
                    &mut entities_map,
                    mention_text,
                    normalized,
                    ExtractedEntityType::Mention,
                    position,
                );
                total_mentions += 1;
            }

            // Bound check
            if entities_map.len() >= config.max_entities {
                break;
            }
        }
    }

    // Extract emails
    if config.extract_emails && entities_map.len() < config.max_entities {
        for cap in EMAIL_PATTERN.find_iter(content) {
            let email = cap.as_str();
            let normalized = normalize_alias(email);

            add_or_increment(
                &mut entities_map,
                email.to_string(),
                normalized,
                ExtractedEntityType::Email,
                cap.start(),
            );
            total_mentions += 1;

            if entities_map.len() >= config.max_entities {
                break;
            }
        }
    }

    // Extract URLs (just domain part)
    if config.extract_urls && entities_map.len() < config.max_entities {
        for cap in URL_PATTERN.captures_iter(content) {
            if let Some(domain) = cap.get(1) {
                let domain_str = domain.as_str();
                // Skip common domains
                if !is_common_domain(domain_str) {
                    let normalized = normalize_alias(domain_str);

                    add_or_increment(
                        &mut entities_map,
                        domain_str.to_string(),
                        normalized,
                        ExtractedEntityType::Url,
                        cap.get(0).map(|c| c.start()).unwrap_or(0),
                    );
                    total_mentions += 1;
                }
            }

            if entities_map.len() >= config.max_entities {
                break;
            }
        }
    }

    // Extract capitalized names
    if config.extract_names && entities_map.len() < config.max_entities {
        for cap in NAME_PATTERN.find_iter(content) {
            let name = cap.as_str();
            // Skip common phrases
            if !is_common_phrase(name) {
                let normalized = normalize_alias(name);

                add_or_increment(
                    &mut entities_map,
                    name.to_string(),
                    normalized,
                    ExtractedEntityType::Name,
                    cap.start(),
                );
                total_mentions += 1;
            }

            if entities_map.len() >= config.max_entities {
                break;
            }
        }
    }

    // Resolve entities against existing identities
    let mut resolved_count = 0;
    if config.lookup_aliases {
        if let Some(conn) = conn {
            for entity in entities_map.values_mut() {
                if let Ok(Some(identity)) = resolve_alias(conn, &entity.normalized) {
                    entity.resolved_id = Some(identity.canonical_id);
                    entity.entity_type = ExtractedEntityType::KnownAlias;
                    entity.confidence = 1.0;
                    resolved_count += 1;
                }
            }
        }
    }

    // Filter by confidence and convert to vec
    let mut entities: Vec<ExtractedEntity> = entities_map
        .into_values()
        .filter(|e| e.confidence >= config.min_confidence)
        .collect();

    // Sort by position for stable output
    entities.sort_by_key(|e| e.position);

    // Truncate to max
    entities.truncate(config.max_entities);

    debug!(
        entity_count = entities.len(),
        total_mentions, resolved_count, "Entity extraction complete"
    );

    ExtractionResult {
        entities,
        total_mentions,
        resolved_count,
    }
}

/// Add entity or increment count if exists
fn add_or_increment(
    map: &mut HashMap<String, ExtractedEntity>,
    mention_text: String,
    normalized: String,
    entity_type: ExtractedEntityType,
    position: usize,
) {
    if let Some(existing) = map.get_mut(&normalized) {
        existing.count += 1;
    } else {
        map.insert(
            normalized.clone(),
            ExtractedEntity {
                mention_text,
                normalized,
                entity_type,
                confidence: entity_type.default_confidence(),
                position,
                count: 1,
                resolved_id: None,
            },
        );
    }
}

/// Check if domain is too common to be meaningful
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

/// Check if phrase is too common to be a name
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

/// Auto-link entities found in a memory's content to identities.
///
/// This function:
/// 1. Extracts entities from content
/// 2. Creates new identities for unresolved entities (optional)
/// 3. Links all resolved entities to the memory
///
/// # Arguments
/// * `conn` - Database connection
/// * `memory_id` - Memory to link entities to
/// * `content` - Memory content to extract from
/// * `auto_create` - Whether to create new identities for unresolved entities
///
/// # Returns
/// Number of entities linked
#[instrument(skip(conn, content), fields(memory_id, auto_create, content_len = content.len()))]
pub fn auto_link_memory(
    conn: &Connection,
    memory_id: i64,
    content: &str,
    auto_create: bool,
) -> Result<usize> {
    use crate::storage::identity_links::{
        create_identity, link_identity_to_memory, CreateIdentityInput, IdentityType,
    };

    let config = ExtractionConfig::default();
    let result = extract_entities(content, &config, Some(conn));

    let mut linked_count = 0;

    for entity in result.entities {
        let canonical_id = if let Some(id) = entity.resolved_id {
            // Already resolved to existing identity
            id
        } else if auto_create {
            // Create new identity for this entity
            let entity_type = match entity.entity_type {
                ExtractedEntityType::Email => IdentityType::Person,
                ExtractedEntityType::Mention => IdentityType::Person,
                ExtractedEntityType::Url => IdentityType::Organization,
                ExtractedEntityType::Name => IdentityType::Person,
                ExtractedEntityType::KnownAlias => IdentityType::Other,
            };

            let input = CreateIdentityInput {
                canonical_id: format!("auto:{}", entity.normalized),
                display_name: entity.mention_text.clone(),
                entity_type,
                description: Some("Auto-created from entity extraction".to_string()),
                metadata: HashMap::new(),
                aliases: vec![entity.mention_text.clone()],
            };

            match create_identity(conn, &input) {
                Ok(identity) => identity.canonical_id,
                Err(_) => continue, // Skip if creation fails (e.g., already exists)
            }
        } else {
            continue; // Skip unresolved entities
        };

        // Link to memory
        if link_identity_to_memory(conn, memory_id, &canonical_id, Some(&entity.mention_text))
            .is_ok()
        {
            linked_count += 1;
        }
    }

    Ok(linked_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mentions() {
        let config = ExtractionConfig {
            lookup_aliases: false,
            ..Default::default()
        };

        let result = extract_entities("Hello @alice and @bob-smith!", &config, None);

        assert_eq!(result.entities.len(), 2);
        assert_eq!(result.entities[0].mention_text, "@alice");
        assert_eq!(result.entities[1].mention_text, "@bob-smith");
    }

    #[test]
    fn test_extract_emails() {
        let config = ExtractionConfig {
            lookup_aliases: false,
            extract_names: false,
            extract_mentions: false,
            extract_urls: false,
            extract_emails: true,
            ..Default::default()
        };

        let result = extract_entities("Contact us at hello@example.com", &config, None);

        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].mention_text, "hello@example.com");
        assert_eq!(result.entities[0].entity_type, ExtractedEntityType::Email);
    }

    #[test]
    fn test_extract_names() {
        let config = ExtractionConfig {
            lookup_aliases: false,
            ..Default::default()
        };

        let result = extract_entities("I met John Smith yesterday", &config, None);

        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].mention_text, "John Smith");
        assert_eq!(result.entities[0].entity_type, ExtractedEntityType::Name);
    }

    #[test]
    fn test_empty_input() {
        let config = ExtractionConfig::default();
        let result = extract_entities("", &config, None);
        assert!(result.entities.is_empty());

        let result = extract_entities("   ", &config, None);
        assert!(result.entities.is_empty());
    }

    #[test]
    fn test_deduplication() {
        let config = ExtractionConfig {
            lookup_aliases: false,
            ..Default::default()
        };

        let result = extract_entities("@alice said hello. @alice waved.", &config, None);

        assert_eq!(result.entities.len(), 1);
        assert_eq!(result.entities[0].count, 2);
        assert_eq!(result.total_mentions, 2);
    }

    #[test]
    fn test_max_entities_bound() {
        let config = ExtractionConfig {
            lookup_aliases: false,
            max_entities: 2,
            ..Default::default()
        };

        let result = extract_entities("@a @b @c @d @e", &config, None);

        assert!(result.entities.len() <= 2);
    }

    #[test]
    fn test_normalization_invariant() {
        // Invariant: normalize_alias is idempotent
        let inputs = vec![
            "@Alice",
            "  bob  ",
            "@CHARLIE",
            "user@email.com",
            "  @mixed  CASE  ",
        ];

        for input in inputs {
            let once = normalize_alias(input);
            let twice = normalize_alias(&once);
            assert_eq!(
                once, twice,
                "Normalization should be idempotent for: {}",
                input
            );
        }
    }

    #[test]
    fn test_never_panics_on_bad_input() {
        let config = ExtractionConfig {
            lookup_aliases: false,
            ..Default::default()
        };

        // Pre-allocate strings that need longer lifetime
        let long_a = "a".repeat(10000);
        let long_at = "@".repeat(1000);

        // Various edge cases that shouldn't panic
        let inputs: Vec<&str> = vec![
            "",
            "   ",
            "@",
            "@@@@",
            "@a",
            "a@",
            "http://",
            "https://",
            &long_a,
            &long_at,
            "\0\0\0",
            "emoji: 🎉🎊🎁",
            "unicode: 日本語 中文 한국어",
        ];

        for input in inputs {
            let result = extract_entities(input, &config, None);
            // Just verify no panic
            let _ = result.entities.len();
        }
    }
}
