//! AI Auto-Tagging for Memories
//!
//! Automatically suggests and applies tags to memories based on content analysis.
//! Uses multiple strategies: keyword extraction, pattern matching, and entity detection.

use crate::types::{Memory, MemoryType};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Configuration for auto-tagging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTagConfig {
    /// Minimum confidence to suggest a tag
    pub min_confidence: f32,
    /// Maximum number of tags to suggest
    pub max_tags: usize,
    /// Enable pattern-based tagging
    pub enable_patterns: bool,
    /// Enable keyword-based tagging
    pub enable_keywords: bool,
    /// Enable entity-based tagging (uses NER)
    pub enable_entities: bool,
    /// Enable type-based tagging
    pub enable_type_tags: bool,
    /// Custom keyword-to-tag mappings
    pub keyword_mappings: HashMap<String, String>,
}

impl Default for AutoTagConfig {
    fn default() -> Self {
        Self {
            min_confidence: 0.5,
            max_tags: 5,
            enable_patterns: true,
            enable_keywords: true,
            enable_entities: true,
            enable_type_tags: true,
            keyword_mappings: HashMap::new(),
        }
    }
}

/// A suggested tag with confidence score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSuggestion {
    /// The suggested tag
    pub tag: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Source of the suggestion
    pub source: TagSource,
    /// Reason for the suggestion
    pub reason: String,
}

impl TagSuggestion {
    pub fn new(
        tag: impl Into<String>,
        confidence: f32,
        source: TagSource,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            tag: tag.into(),
            confidence,
            source,
            reason: reason.into(),
        }
    }
}

/// Source of a tag suggestion
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagSource {
    /// Extracted from keywords in content
    Keyword,
    /// Matched a known pattern
    Pattern,
    /// Derived from memory type
    MemoryType,
    /// Based on detected entities
    Entity,
    /// Based on content structure
    Structure,
    /// User-defined mapping
    Custom,
}

/// Result of auto-tagging analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoTagResult {
    /// Suggested tags (sorted by confidence)
    pub suggestions: Vec<TagSuggestion>,
    /// Tags that were automatically applied (if auto_apply was true)
    pub applied_tags: Vec<String>,
    /// Number of tags analyzed
    pub analysis_count: usize,
}

/// Auto-tagging engine
pub struct AutoTagger {
    config: AutoTagConfig,
    /// Predefined patterns for tag extraction
    patterns: Vec<TagPattern>,
    /// Technical keywords mapped to tags
    tech_keywords: HashMap<&'static str, &'static str>,
    /// Action keywords
    action_keywords: HashMap<&'static str, &'static str>,
}

/// Pattern for matching content to tags
struct TagPattern {
    /// Regex-like pattern (simplified)
    keywords: Vec<&'static str>,
    /// Tag to apply
    tag: &'static str,
    /// Base confidence
    confidence: f32,
}

impl Default for AutoTagger {
    fn default() -> Self {
        Self::new(AutoTagConfig::default())
    }
}

impl AutoTagger {
    /// Create a new auto-tagger
    pub fn new(config: AutoTagConfig) -> Self {
        Self {
            config,
            patterns: Self::default_patterns(),
            tech_keywords: Self::default_tech_keywords(),
            action_keywords: Self::default_action_keywords(),
        }
    }

    fn default_patterns() -> Vec<TagPattern> {
        vec![
            // Code-related patterns
            TagPattern {
                keywords: vec!["function", "method", "class", "struct"],
                tag: "code",
                confidence: 0.8,
            },
            TagPattern {
                keywords: vec!["bug", "fix", "issue", "error"],
                tag: "bug",
                confidence: 0.85,
            },
            TagPattern {
                keywords: vec!["test", "testing", "unit test", "integration"],
                tag: "testing",
                confidence: 0.8,
            },
            TagPattern {
                keywords: vec!["deploy", "deployment", "release", "production"],
                tag: "deployment",
                confidence: 0.8,
            },
            TagPattern {
                keywords: vec![
                    "security",
                    "auth",
                    "authentication",
                    "authorization",
                    "permission",
                ],
                tag: "security",
                confidence: 0.85,
            },
            TagPattern {
                keywords: vec!["api", "endpoint", "rest", "graphql", "grpc"],
                tag: "api",
                confidence: 0.8,
            },
            TagPattern {
                keywords: vec!["database", "sql", "query", "migration", "schema"],
                tag: "database",
                confidence: 0.8,
            },
            // Documentation patterns
            TagPattern {
                keywords: vec!["documentation", "docs", "readme", "guide"],
                tag: "documentation",
                confidence: 0.8,
            },
            // Architecture patterns
            TagPattern {
                keywords: vec!["architecture", "design", "pattern", "structure"],
                tag: "architecture",
                confidence: 0.75,
            },
            // Decision patterns
            TagPattern {
                keywords: vec!["decided", "decision", "agreed", "consensus"],
                tag: "decision",
                confidence: 0.9,
            },
            // Learning patterns
            TagPattern {
                keywords: vec!["learned", "til", "today i learned", "insight"],
                tag: "learning",
                confidence: 0.85,
            },
            // Meeting patterns
            TagPattern {
                keywords: vec!["meeting", "standup", "sync", "discussion"],
                tag: "meeting",
                confidence: 0.8,
            },
            // Performance patterns
            TagPattern {
                keywords: vec!["performance", "optimization", "benchmark", "speed"],
                tag: "performance",
                confidence: 0.8,
            },
            // Config patterns
            TagPattern {
                keywords: vec!["config", "configuration", "settings", "environment"],
                tag: "config",
                confidence: 0.75,
            },
        ]
    }

    fn default_tech_keywords() -> HashMap<&'static str, &'static str> {
        let mut map = HashMap::new();
        // Programming languages
        map.insert("rust", "lang/rust");
        map.insert("python", "lang/python");
        map.insert("javascript", "lang/javascript");
        map.insert("typescript", "lang/typescript");
        map.insert("go", "lang/go");
        map.insert("golang", "lang/go");
        map.insert("java", "lang/java");
        map.insert("kotlin", "lang/kotlin");
        map.insert("swift", "lang/swift");
        map.insert("c++", "lang/cpp");
        map.insert("ruby", "lang/ruby");

        // Frameworks
        map.insert("react", "framework/react");
        map.insert("nextjs", "framework/nextjs");
        map.insert("next.js", "framework/nextjs");
        map.insert("vue", "framework/vue");
        map.insert("angular", "framework/angular");
        map.insert("django", "framework/django");
        map.insert("fastapi", "framework/fastapi");
        map.insert("axum", "framework/axum");
        map.insert("actix", "framework/actix");
        map.insert("express", "framework/express");

        // Databases
        map.insert("postgresql", "db/postgres");
        map.insert("postgres", "db/postgres");
        map.insert("mysql", "db/mysql");
        map.insert("mongodb", "db/mongodb");
        map.insert("redis", "db/redis");
        map.insert("sqlite", "db/sqlite");

        // Cloud/Infra
        map.insert("aws", "cloud/aws");
        map.insert("gcp", "cloud/gcp");
        map.insert("azure", "cloud/azure");
        map.insert("cloudflare", "cloud/cloudflare");
        map.insert("docker", "infra/docker");
        map.insert("kubernetes", "infra/kubernetes");
        map.insert("k8s", "infra/kubernetes");
        map.insert("terraform", "infra/terraform");

        map
    }

    fn default_action_keywords() -> HashMap<&'static str, &'static str> {
        let mut map = HashMap::new();
        map.insert("todo", "action/todo");
        map.insert("fixme", "action/fixme");
        map.insert("hack", "action/hack");
        map.insert("review", "action/review");
        map.insert("refactor", "action/refactor");
        map.insert("optimize", "action/optimize");
        map.insert("deprecate", "action/deprecate");
        map
    }

    /// Analyze content and suggest tags
    pub fn suggest_tags(
        &self,
        content: &str,
        memory_type: Option<MemoryType>,
        existing_tags: &[String],
    ) -> AutoTagResult {
        let mut suggestions: Vec<TagSuggestion> = Vec::new();
        let content_lower = content.to_lowercase();
        let existing_set: HashSet<_> = existing_tags.iter().map(|t| t.to_lowercase()).collect();

        // Pattern-based tagging
        if self.config.enable_patterns {
            for pattern in &self.patterns {
                let matches: usize = pattern
                    .keywords
                    .iter()
                    .filter(|kw| content_lower.contains(*kw))
                    .count();

                if matches > 0 {
                    let confidence = pattern.confidence
                        * (matches as f32 / pattern.keywords.len() as f32).min(1.0);
                    if confidence >= self.config.min_confidence
                        && !existing_set.contains(pattern.tag)
                    {
                        suggestions.push(TagSuggestion::new(
                            pattern.tag,
                            confidence,
                            TagSource::Pattern,
                            format!(
                                "Matched {} of {} pattern keywords",
                                matches,
                                pattern.keywords.len()
                            ),
                        ));
                    }
                }
            }
        }

        // Keyword-based tagging (tech and action)
        if self.config.enable_keywords {
            // Tech keywords
            for (keyword, tag) in &self.tech_keywords {
                if content_lower.contains(keyword) && !existing_set.contains(&tag.to_lowercase()) {
                    suggestions.push(TagSuggestion::new(
                        *tag,
                        0.75,
                        TagSource::Keyword,
                        format!("Contains technology keyword: {}", keyword),
                    ));
                }
            }

            // Action keywords
            for (keyword, tag) in &self.action_keywords {
                if content_lower.contains(keyword) && !existing_set.contains(&tag.to_lowercase()) {
                    suggestions.push(TagSuggestion::new(
                        *tag,
                        0.8,
                        TagSource::Keyword,
                        format!("Contains action keyword: {}", keyword),
                    ));
                }
            }

            // Custom keyword mappings
            for (keyword, tag) in &self.config.keyword_mappings {
                if content_lower.contains(&keyword.to_lowercase())
                    && !existing_set.contains(&tag.to_lowercase())
                {
                    suggestions.push(TagSuggestion::new(
                        tag,
                        0.9,
                        TagSource::Custom,
                        format!("Custom mapping: {} -> {}", keyword, tag),
                    ));
                }
            }
        }

        // Memory type based tagging
        if self.config.enable_type_tags {
            if let Some(mem_type) = memory_type {
                let type_tag = match mem_type {
                    MemoryType::Todo => Some("type/todo"),
                    MemoryType::Issue => Some("type/issue"),
                    MemoryType::Decision => Some("type/decision"),
                    MemoryType::Learning => Some("type/learning"),
                    MemoryType::Preference => Some("type/preference"),
                    MemoryType::Context => Some("type/context"),
                    MemoryType::Credential => Some("type/credential"),
                    _ => None,
                };

                if let Some(tag) = type_tag {
                    if !existing_set.contains(&tag.to_lowercase()) {
                        suggestions.push(TagSuggestion::new(
                            tag,
                            0.95,
                            TagSource::MemoryType,
                            format!("Based on memory type: {:?}", mem_type),
                        ));
                    }
                }
            }
        }

        // Structure-based tagging
        if content.contains("```") && !existing_set.contains("has-code") {
            suggestions.push(TagSuggestion::new(
                "has-code",
                0.9,
                TagSource::Structure,
                "Contains code blocks",
            ));
        }

        if (content.contains("http://") || content.contains("https://"))
            && !existing_set.contains("has-links")
        {
            suggestions.push(TagSuggestion::new(
                "has-links",
                0.85,
                TagSource::Structure,
                "Contains URLs",
            ));
        }

        // Deduplicate by tag name, keeping highest confidence
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut deduped: Vec<TagSuggestion> = Vec::new();

        for suggestion in suggestions {
            let tag_lower = suggestion.tag.to_lowercase();
            if let Some(&idx) = seen.get(&tag_lower) {
                if suggestion.confidence > deduped[idx].confidence {
                    deduped[idx] = suggestion;
                }
            } else {
                seen.insert(tag_lower, deduped.len());
                deduped.push(suggestion);
            }
        }

        // Sort by confidence and limit
        deduped.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        deduped.truncate(self.config.max_tags);

        let analysis_count = deduped.len();

        AutoTagResult {
            suggestions: deduped,
            applied_tags: Vec::new(),
            analysis_count,
        }
    }

    /// Suggest tags for a memory
    pub fn suggest_for_memory(&self, memory: &Memory) -> AutoTagResult {
        self.suggest_tags(&memory.content, Some(memory.memory_type), &memory.tags)
    }

    /// Get configuration
    pub fn config(&self) -> &AutoTagConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: AutoTagConfig) {
        self.config = config;
    }

    /// Add custom keyword mapping
    pub fn add_keyword_mapping(&mut self, keyword: impl Into<String>, tag: impl Into<String>) {
        self.config
            .keyword_mappings
            .insert(keyword.into(), tag.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_matching() {
        let mut config = AutoTagConfig::default();
        config.max_tags = 10;
        config.min_confidence = 0.1; // Lower threshold to test pattern matching
        let tagger = AutoTagger::new(config);

        let result = tagger.suggest_tags(
            "We decided to use PostgreSQL for the database and implement authentication with JWT",
            None,
            &[],
        );

        let tags: Vec<_> = result.suggestions.iter().map(|s| s.tag.as_str()).collect();

        assert!(tags.contains(&"decision"));
        assert!(tags.contains(&"database"));
        assert!(tags.contains(&"security"));
        assert!(tags.contains(&"db/postgres"));
    }

    #[test]
    fn test_tech_keyword_detection() {
        let mut config = AutoTagConfig::default();
        config.max_tags = 10;
        config.min_confidence = 0.1; // Lower threshold to test keyword matching
        let tagger = AutoTagger::new(config);

        let result = tagger.suggest_tags(
            "Building a REST API with Rust and Axum framework",
            None,
            &[],
        );

        let tags: Vec<_> = result.suggestions.iter().map(|s| s.tag.as_str()).collect();

        assert!(tags.contains(&"lang/rust"));
        assert!(tags.contains(&"framework/axum"));
        assert!(tags.contains(&"api"));
    }

    #[test]
    fn test_action_keywords() {
        let mut config = AutoTagConfig::default();
        config.max_tags = 10;
        config.min_confidence = 0.1; // Lower threshold to test keyword matching
        let tagger = AutoTagger::new(config);

        let result = tagger.suggest_tags(
            "TODO: refactor this function to improve performance",
            None,
            &[],
        );

        let tags: Vec<_> = result.suggestions.iter().map(|s| s.tag.as_str()).collect();

        assert!(tags.contains(&"action/todo"));
        assert!(tags.contains(&"action/refactor"));
        assert!(tags.contains(&"performance"));
    }

    #[test]
    fn test_structure_detection() {
        let tagger = AutoTagger::default();

        let result = tagger.suggest_tags(
            "Here's the code:\n```rust\nfn main() {}\n```\nAnd docs: https://docs.rs",
            None,
            &[],
        );

        let tags: Vec<_> = result.suggestions.iter().map(|s| s.tag.as_str()).collect();

        assert!(tags.contains(&"has-code"));
        assert!(tags.contains(&"has-links"));
    }

    #[test]
    fn test_excludes_existing_tags() {
        let tagger = AutoTagger::default();

        let result = tagger.suggest_tags(
            "We decided to use PostgreSQL",
            None,
            &["decision".to_string()],
        );

        let tags: Vec<_> = result.suggestions.iter().map(|s| s.tag.as_str()).collect();

        // Should not suggest "decision" since it's already present
        assert!(!tags.contains(&"decision"));
        // But should still suggest database-related
        assert!(tags.contains(&"db/postgres") || tags.contains(&"database"));
    }

    #[test]
    fn test_memory_type_tagging() {
        let tagger = AutoTagger::default();

        let result =
            tagger.suggest_tags("Remember to update the docs", Some(MemoryType::Todo), &[]);

        let tags: Vec<_> = result.suggestions.iter().map(|s| s.tag.as_str()).collect();

        assert!(tags.contains(&"type/todo"));
    }

    #[test]
    fn test_custom_mappings() {
        let mut tagger = AutoTagger::default();
        tagger.add_keyword_mapping("ibvi", "project/ibvi");
        tagger.add_keyword_mapping("mbras", "project/mbras");

        let result = tagger.suggest_tags("Working on the IBVI dashboard feature", None, &[]);

        let tags: Vec<_> = result.suggestions.iter().map(|s| s.tag.as_str()).collect();

        assert!(tags.contains(&"project/ibvi"));
    }

    #[test]
    fn test_confidence_sorting() {
        let tagger = AutoTagger::default();

        let result = tagger.suggest_tags(
            "We decided to fix the bug in the authentication system using Rust",
            None,
            &[],
        );

        // Suggestions should be sorted by confidence (descending)
        for i in 1..result.suggestions.len() {
            assert!(result.suggestions[i - 1].confidence >= result.suggestions[i].confidence);
        }
    }

    #[test]
    fn test_max_tags_limit() {
        let mut config = AutoTagConfig::default();
        config.max_tags = 3;
        let tagger = AutoTagger::new(config);

        let result = tagger.suggest_tags(
            "We decided to fix the bug in the authentication system using Rust with PostgreSQL database and deploy to AWS using Docker",
            None,
            &[],
        );

        assert!(result.suggestions.len() <= 3);
    }
}
