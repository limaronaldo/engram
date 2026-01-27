//! Entity Extraction for Engram (RML-925)
//!
//! Provides automatic Named Entity Recognition (NER) to extract:
//! - People (names, roles, mentions)
//! - Organizations (companies, teams)
//! - Projects (repos, products)
//! - Concepts (technical terms, patterns)
//! - Locations (places, regions)
//! - Dates/Times (temporal references)
//!
//! Uses pattern-based extraction (fast, no dependencies) with optional
//! LLM-enhanced extraction for higher quality.

use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::types::MemoryId;

// =============================================================================
// Types
// =============================================================================

/// Type of entity extracted from text
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityType {
    /// Person name (e.g., "John Smith", "@username")
    Person,
    /// Organization or company (e.g., "Anthropic", "OpenAI")
    Organization,
    /// Project or repository (e.g., "engram", "rust-analyzer")
    Project,
    /// Technical concept or term (e.g., "vector database", "embeddings")
    Concept,
    /// Geographic location (e.g., "San Francisco", "AWS us-east-1")
    Location,
    /// Date or time reference (e.g., "yesterday", "Q4 2024")
    DateTime,
    /// URL or file path
    Reference,
    /// Generic/unknown entity type
    Other,
}

impl EntityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityType::Person => "person",
            EntityType::Organization => "organization",
            EntityType::Project => "project",
            EntityType::Concept => "concept",
            EntityType::Location => "location",
            EntityType::DateTime => "datetime",
            EntityType::Reference => "reference",
            EntityType::Other => "other",
        }
    }
}

impl std::str::FromStr for EntityType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "person" => Ok(EntityType::Person),
            "organization" | "org" | "company" => Ok(EntityType::Organization),
            "project" | "repo" | "repository" => Ok(EntityType::Project),
            "concept" | "term" | "topic" => Ok(EntityType::Concept),
            "location" | "place" | "geo" => Ok(EntityType::Location),
            "datetime" | "date" | "time" => Ok(EntityType::DateTime),
            "reference" | "url" | "path" => Ok(EntityType::Reference),
            "other" => Ok(EntityType::Other),
            _ => Err(format!("Unknown entity type: {}", s)),
        }
    }
}

/// An extracted entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    /// Unique identifier
    pub id: i64,
    /// Canonical name of the entity
    pub name: String,
    /// Normalized name for matching (lowercase, trimmed)
    pub normalized_name: String,
    /// Type of entity
    pub entity_type: EntityType,
    /// Aliases (other names this entity is known by)
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// When first seen
    pub created_at: DateTime<Utc>,
    /// When last referenced
    pub updated_at: DateTime<Utc>,
    /// Number of times referenced
    #[serde(default)]
    pub mention_count: i32,
}

/// Relationship between a memory and an entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntity {
    /// Memory ID
    pub memory_id: MemoryId,
    /// Entity ID
    pub entity_id: i64,
    /// Type of relation (mentions, defines, references, etc.)
    pub relation: EntityRelation,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Character offset where entity appears in content
    pub offset: Option<usize>,
    /// When the link was created
    pub created_at: DateTime<Utc>,
}

/// Type of relationship between memory and entity
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityRelation {
    /// Entity is mentioned in the memory
    Mentions,
    /// Memory defines or describes the entity
    Defines,
    /// Memory references the entity (e.g., link, citation)
    References,
    /// Memory is about/focuses on the entity
    About,
    /// Memory was created by the entity (for Person type)
    CreatedBy,
}

impl EntityRelation {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityRelation::Mentions => "mentions",
            EntityRelation::Defines => "defines",
            EntityRelation::References => "references",
            EntityRelation::About => "about",
            EntityRelation::CreatedBy => "created_by",
        }
    }
}

impl std::str::FromStr for EntityRelation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mentions" => Ok(EntityRelation::Mentions),
            "defines" => Ok(EntityRelation::Defines),
            "references" => Ok(EntityRelation::References),
            "about" => Ok(EntityRelation::About),
            "created_by" | "createdby" => Ok(EntityRelation::CreatedBy),
            _ => Err(format!("Unknown entity relation: {}", s)),
        }
    }
}

/// Result of entity extraction from text
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    /// Extracted entities with their positions
    pub entities: Vec<ExtractedEntity>,
    /// Total extraction time in milliseconds
    pub extraction_time_ms: u64,
}

/// A single extracted entity from text
#[derive(Debug, Clone)]
pub struct ExtractedEntity {
    /// The extracted text
    pub text: String,
    /// Normalized form
    pub normalized: String,
    /// Entity type
    pub entity_type: EntityType,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Character offset in source text
    pub offset: usize,
    /// Length of the match
    pub length: usize,
    /// Suggested relation type
    pub suggested_relation: EntityRelation,
}

// =============================================================================
// Entity Extraction Engine
// =============================================================================

/// Configuration for entity extraction
#[derive(Debug, Clone)]
pub struct EntityExtractionConfig {
    /// Minimum confidence threshold for extraction
    pub min_confidence: f32,
    /// Extract people names
    pub extract_people: bool,
    /// Extract organizations
    pub extract_organizations: bool,
    /// Extract projects
    pub extract_projects: bool,
    /// Extract concepts
    pub extract_concepts: bool,
    /// Extract locations
    pub extract_locations: bool,
    /// Extract datetime references
    pub extract_datetime: bool,
    /// Extract URLs and paths
    pub extract_references: bool,
    /// Custom patterns to match (name -> entity_type)
    pub custom_patterns: HashMap<String, EntityType>,
}

impl Default for EntityExtractionConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            extract_people: true,
            extract_organizations: true,
            extract_projects: true,
            extract_concepts: true,
            extract_locations: true,
            extract_datetime: true,
            extract_references: true,
            custom_patterns: HashMap::new(),
        }
    }
}

/// Entity extraction engine using pattern matching
pub struct EntityExtractor {
    config: EntityExtractionConfig,
    // Compiled regex patterns
    person_pattern: Regex,
    org_pattern: Regex,
    project_pattern: Regex,
    url_pattern: Regex,
    path_pattern: Regex,
    datetime_pattern: Regex,
    mention_pattern: Regex,
    // Known entities for matching
    known_organizations: HashSet<String>,
    known_concepts: HashSet<String>,
}

// Compiled regex patterns
static PERSON_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        @[\w-]+                           # @username mentions
        |(?:Mr\.|Mrs\.|Ms\.|Dr\.|Prof\.)\s+[A-Z][a-z]+(?:\s+[A-Z][a-z]+)?  # Title + name
        |[A-Z][a-z]+\s+[A-Z][a-z]+(?:\s+[A-Z][a-z]+)?  # First Last (Middle)
        ",
    )
    .unwrap()
});

static ORG_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        [A-Z][A-Za-z]*(?:\s+[A-Z][A-Za-z]*)*\s+(?:Inc\.?|Corp\.?|LLC|Ltd\.?|Co\.?|Team|Group|Labs?)
        |(?:The\s+)?[A-Z][A-Za-z]+(?:\s+[A-Z][A-Za-z]+)*\s+(?:Company|Organization|Foundation|Institute)
        ",
    )
    .unwrap()
});

static PROJECT_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        [a-z][a-z0-9]*(?:-[a-z0-9]+)+     # kebab-case project names
        |[a-z][a-z0-9]*(?:_[a-z0-9]+)+    # snake_case project names
        |[A-Z][a-z]+(?:[A-Z][a-z]+)+      # PascalCase project names
        |v?\d+\.\d+(?:\.\d+)?(?:-[a-z]+)? # version numbers
        ",
    )
    .unwrap()
});

static URL_PATTERN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"https?://[^\s<>\[\]()]+|www\.[^\s<>\[\]]+").unwrap());

static PATH_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        (?:/[\w.-]+)+                     # Unix paths
        |[A-Z]:\\(?:[\w.-]+\\)+[\w.-]*    # Windows paths
        |\.{1,2}/[\w.-/]+                 # Relative paths
        ",
    )
    .unwrap()
});

static DATETIME_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        \d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}(?::\d{2})?)?  # ISO dates
        |\d{1,2}/\d{1,2}/\d{2,4}          # MM/DD/YYYY
        |(?:Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]*\.?\s+\d{1,2}(?:,?\s+\d{4})?
        |Q[1-4]\s+\d{4}                   # Quarters
        |(?:yesterday|today|tomorrow|last\s+week|next\s+month)
        ",
    )
    .unwrap()
});

static MENTION_PATTERN: Lazy<Regex> = Lazy::new(|| Regex::new(r"@[\w-]+").unwrap());

static KNOWN_ORGANIZATIONS: Lazy<HashSet<String>> = Lazy::new(|| {
    [
        "Anthropic",
        "OpenAI",
        "Google",
        "Microsoft",
        "Meta",
        "Amazon",
        "Apple",
        "GitHub",
        "GitLab",
        "Vercel",
        "Cloudflare",
        "AWS",
        "Azure",
        "GCP",
        "Stripe",
        "Supabase",
        "Neon",
        "PlanetScale",
        "MongoDB",
        "Redis",
    ]
    .iter()
    .map(|s| s.to_lowercase())
    .collect()
});

static KNOWN_CONCEPTS: Lazy<HashSet<String>> = Lazy::new(|| {
    [
        "machine learning",
        "deep learning",
        "neural network",
        "transformer",
        "embedding",
        "vector database",
        "semantic search",
        "rag",
        "llm",
        "api",
        "rest",
        "graphql",
        "grpc",
        "websocket",
        "microservices",
        "kubernetes",
        "docker",
        "ci/cd",
        "devops",
        "serverless",
        "authentication",
        "authorization",
        "oauth",
        "jwt",
        "session",
        "database",
        "sql",
        "nosql",
        "postgresql",
        "sqlite",
        "redis",
        "rust",
        "python",
        "typescript",
        "javascript",
        "go",
        "java",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
});

impl EntityExtractor {
    pub fn new(config: EntityExtractionConfig) -> Self {
        Self {
            config,
            person_pattern: PERSON_PATTERN.clone(),
            org_pattern: ORG_PATTERN.clone(),
            project_pattern: PROJECT_PATTERN.clone(),
            url_pattern: URL_PATTERN.clone(),
            path_pattern: PATH_PATTERN.clone(),
            datetime_pattern: DATETIME_PATTERN.clone(),
            mention_pattern: MENTION_PATTERN.clone(),
            known_organizations: KNOWN_ORGANIZATIONS.clone(),
            known_concepts: KNOWN_CONCEPTS.clone(),
        }
    }

    /// Extract entities from text
    pub fn extract(&self, text: &str) -> ExtractionResult {
        let start = std::time::Instant::now();
        let mut entities = Vec::new();
        let text_lower = text.to_lowercase();

        // Extract @mentions (high confidence)
        if self.config.extract_people {
            for cap in self.mention_pattern.find_iter(text) {
                entities.push(ExtractedEntity {
                    text: cap.as_str().to_string(),
                    normalized: cap.as_str().to_lowercase(),
                    entity_type: EntityType::Person,
                    confidence: 0.95,
                    offset: cap.start(),
                    length: cap.len(),
                    suggested_relation: EntityRelation::Mentions,
                });
            }

            // Extract person names
            for cap in self.person_pattern.find_iter(text) {
                // Skip if already captured as @mention
                if cap.as_str().starts_with('@') {
                    continue;
                }
                entities.push(ExtractedEntity {
                    text: cap.as_str().to_string(),
                    normalized: normalize_name(cap.as_str()),
                    entity_type: EntityType::Person,
                    confidence: 0.7,
                    offset: cap.start(),
                    length: cap.len(),
                    suggested_relation: EntityRelation::Mentions,
                });
            }
        }

        // Extract organizations
        if self.config.extract_organizations {
            for cap in self.org_pattern.find_iter(text) {
                entities.push(ExtractedEntity {
                    text: cap.as_str().to_string(),
                    normalized: normalize_name(cap.as_str()),
                    entity_type: EntityType::Organization,
                    confidence: 0.8,
                    offset: cap.start(),
                    length: cap.len(),
                    suggested_relation: EntityRelation::Mentions,
                });
            }

            // Check for known organizations
            for org in &self.known_organizations {
                if let Some(pos) = text_lower.find(org) {
                    // Get the original case version
                    let original = &text[pos..pos + org.len()];
                    // Avoid duplicates
                    if !entities.iter().any(|e| e.offset == pos) {
                        entities.push(ExtractedEntity {
                            text: original.to_string(),
                            normalized: org.clone(),
                            entity_type: EntityType::Organization,
                            confidence: 0.9,
                            offset: pos,
                            length: org.len(),
                            suggested_relation: EntityRelation::Mentions,
                        });
                    }
                }
            }
        }

        // Extract URLs
        if self.config.extract_references {
            for cap in self.url_pattern.find_iter(text) {
                entities.push(ExtractedEntity {
                    text: cap.as_str().to_string(),
                    normalized: cap.as_str().to_lowercase(),
                    entity_type: EntityType::Reference,
                    confidence: 0.99,
                    offset: cap.start(),
                    length: cap.len(),
                    suggested_relation: EntityRelation::References,
                });
            }

            for cap in self.path_pattern.find_iter(text) {
                entities.push(ExtractedEntity {
                    text: cap.as_str().to_string(),
                    normalized: cap.as_str().to_string(),
                    entity_type: EntityType::Reference,
                    confidence: 0.85,
                    offset: cap.start(),
                    length: cap.len(),
                    suggested_relation: EntityRelation::References,
                });
            }
        }

        // Extract datetime
        if self.config.extract_datetime {
            for cap in self.datetime_pattern.find_iter(text) {
                entities.push(ExtractedEntity {
                    text: cap.as_str().to_string(),
                    normalized: cap.as_str().to_lowercase(),
                    entity_type: EntityType::DateTime,
                    confidence: 0.9,
                    offset: cap.start(),
                    length: cap.len(),
                    suggested_relation: EntityRelation::Mentions,
                });
            }
        }

        // Extract concepts
        if self.config.extract_concepts {
            for concept in &self.known_concepts {
                if let Some(pos) = text_lower.find(concept) {
                    let original = &text[pos..pos + concept.len()];
                    entities.push(ExtractedEntity {
                        text: original.to_string(),
                        normalized: concept.clone(),
                        entity_type: EntityType::Concept,
                        confidence: 0.85,
                        offset: pos,
                        length: concept.len(),
                        suggested_relation: EntityRelation::About,
                    });
                }
            }
        }

        // Extract project names
        if self.config.extract_projects {
            for cap in self.project_pattern.find_iter(text) {
                let matched = cap.as_str();
                // Skip very short matches and pure version numbers
                if matched.len() < 3
                    || matched
                        .chars()
                        .all(|c| c.is_numeric() || c == '.' || c == '-' || c == 'v')
                {
                    continue;
                }
                entities.push(ExtractedEntity {
                    text: matched.to_string(),
                    normalized: matched.to_lowercase(),
                    entity_type: EntityType::Project,
                    confidence: 0.6,
                    offset: cap.start(),
                    length: cap.len(),
                    suggested_relation: EntityRelation::Mentions,
                });
            }
        }

        // Filter by confidence threshold and deduplicate
        entities.retain(|e| e.confidence >= self.config.min_confidence);
        deduplicate_entities(&mut entities);

        let extraction_time_ms = start.elapsed().as_millis() as u64;

        ExtractionResult {
            entities,
            extraction_time_ms,
        }
    }

    /// Add a custom pattern for entity extraction
    pub fn add_custom_pattern(&mut self, pattern: &str, entity_type: EntityType) {
        self.config
            .custom_patterns
            .insert(pattern.to_string(), entity_type);
    }

    /// Get configuration
    pub fn config(&self) -> &EntityExtractionConfig {
        &self.config
    }
}

impl Default for EntityExtractor {
    fn default() -> Self {
        Self::new(EntityExtractionConfig::default())
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Normalize a name for matching
fn normalize_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deduplicate entities, keeping the highest confidence match
fn deduplicate_entities(entities: &mut Vec<ExtractedEntity>) {
    // Sort by offset, then by confidence (descending)
    entities.sort_by(|a, b| {
        a.offset.cmp(&b.offset).then(
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    // Remove overlapping entities, keeping higher confidence
    let mut i = 0;
    while i < entities.len() {
        let current_end = entities[i].offset + entities[i].length;
        let mut j = i + 1;
        while j < entities.len() {
            if entities[j].offset < current_end {
                // Overlapping - remove the lower confidence one
                if entities[j].confidence > entities[i].confidence {
                    entities.remove(i);
                    // Don't increment i, check the new element at position i
                    continue;
                } else {
                    entities.remove(j);
                    // Don't increment j, check the new element at position j
                    continue;
                }
            }
            j += 1;
        }
        i += 1;
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_mentions() {
        let extractor = EntityExtractor::default();
        let result = extractor.extract("Hey @john-doe, can you review this with @alice?");

        let people: Vec<_> = result
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Person)
            .collect();

        assert_eq!(people.len(), 2);
        assert!(people.iter().any(|e| e.text == "@john-doe"));
        assert!(people.iter().any(|e| e.text == "@alice"));
    }

    #[test]
    fn test_extract_urls() {
        let extractor = EntityExtractor::default();
        let result = extractor.extract("Check out https://github.com/engram/engram for more info.");

        let refs: Vec<_> = result
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Reference)
            .collect();

        assert_eq!(refs.len(), 1);
        assert!(refs[0].text.contains("github.com"));
    }

    #[test]
    fn test_extract_organizations() {
        let extractor = EntityExtractor::default();
        let result = extractor.extract("We're using Anthropic's Claude and OpenAI's GPT-4.");

        let orgs: Vec<_> = result
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Organization)
            .collect();

        assert!(orgs.len() >= 2);
    }

    #[test]
    fn test_extract_concepts() {
        let extractor = EntityExtractor::default();
        let result = extractor.extract("We need to implement semantic search with embeddings.");

        let concepts: Vec<_> = result
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Concept)
            .collect();

        assert!(concepts
            .iter()
            .any(|e| e.normalized.contains("semantic search")));
        assert!(concepts.iter().any(|e| e.normalized.contains("embedding")));
    }

    #[test]
    fn test_extract_dates() {
        let extractor = EntityExtractor::default();
        let result = extractor
            .extract("Meeting scheduled for 2024-01-15. Let's discuss yesterday's issues.");

        let dates: Vec<_> = result
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::DateTime)
            .collect();

        assert!(dates.len() >= 2);
    }

    #[test]
    fn test_entity_type_parsing() {
        assert_eq!("person".parse::<EntityType>().unwrap(), EntityType::Person);
        assert_eq!(
            "org".parse::<EntityType>().unwrap(),
            EntityType::Organization
        );
        assert_eq!("repo".parse::<EntityType>().unwrap(), EntityType::Project);
    }

    #[test]
    fn test_confidence_threshold() {
        let config = EntityExtractionConfig {
            min_confidence: 0.9,
            ..Default::default()
        };
        let extractor = EntityExtractor::new(config);

        // Low confidence matches should be filtered out
        let result = extractor.extract("Some random text with John Smith mentioned.");

        // Person names have 0.7 confidence, should be filtered
        let people: Vec<_> = result
            .entities
            .iter()
            .filter(|e| e.entity_type == EntityType::Person && !e.text.starts_with('@'))
            .collect();

        assert!(people.is_empty());
    }
}
