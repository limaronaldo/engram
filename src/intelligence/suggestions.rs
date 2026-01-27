//! Smart Memory Suggestions from Conversation (RML-890)
//!
//! Analyzes conversation context to suggest relevant memories.
//! Uses multiple signals: keyword matching, semantic similarity, recency, and access patterns.

use crate::types::{Memory, SearchResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Type of suggestion
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SuggestionType {
    /// Memory is directly relevant to the current topic
    TopicMatch,
    /// Memory was frequently accessed in similar contexts
    FrequentlyUsed,
    /// Memory is similar to what user is discussing
    SemanticallySimilar,
    /// Memory might be outdated and needs review
    NeedsReview,
    /// Related memory that provides additional context
    RelatedContext,
    /// Memory that contradicts current discussion
    PotentialConflict,
    /// Recently created memory on same topic
    RecentlyAdded,
    /// Memory that user might want to create based on conversation
    SuggestCreate,
}

/// A memory suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Suggestion {
    /// The suggested memory (None for SuggestCreate)
    pub memory: Option<Memory>,
    /// Type of suggestion
    pub suggestion_type: SuggestionType,
    /// Relevance score (0.0 - 1.0)
    pub relevance: f32,
    /// Human-readable reason for the suggestion
    pub reason: String,
    /// Keywords that triggered this suggestion
    pub trigger_keywords: Vec<String>,
    /// Confidence in the suggestion (0.0 - 1.0)
    pub confidence: f32,
    /// Suggested content for SuggestCreate type
    pub suggested_content: Option<String>,
    /// When the suggestion was generated
    pub generated_at: DateTime<Utc>,
}

impl Suggestion {
    /// Create a new suggestion
    pub fn new(
        memory: Option<Memory>,
        suggestion_type: SuggestionType,
        relevance: f32,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            memory,
            suggestion_type,
            relevance,
            reason: reason.into(),
            trigger_keywords: vec![],
            confidence: relevance,
            suggested_content: None,
            generated_at: Utc::now(),
        }
    }

    /// Add trigger keywords
    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.trigger_keywords = keywords;
        self
    }

    /// Set confidence
    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Set suggested content
    pub fn with_suggested_content(mut self, content: impl Into<String>) -> Self {
        self.suggested_content = Some(content.into());
        self
    }
}

/// Configuration for the suggestion engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionConfig {
    /// Maximum number of suggestions to return
    pub max_suggestions: usize,
    /// Minimum relevance score to include
    pub min_relevance: f32,
    /// Weight for recency in scoring
    pub recency_weight: f32,
    /// Weight for access frequency in scoring
    pub frequency_weight: f32,
    /// Weight for semantic similarity in scoring
    pub semantic_weight: f32,
    /// Weight for keyword matching in scoring
    pub keyword_weight: f32,
    /// Days to consider for recency calculations
    pub recency_window_days: i64,
    /// Enable suggest-to-create feature
    pub enable_create_suggestions: bool,
}

impl Default for SuggestionConfig {
    fn default() -> Self {
        Self {
            max_suggestions: 5,
            min_relevance: 0.3,
            recency_weight: 0.2,
            frequency_weight: 0.15,
            semantic_weight: 0.4,
            keyword_weight: 0.25,
            recency_window_days: 30,
            enable_create_suggestions: true,
        }
    }
}

/// Context from conversation for generating suggestions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationContext {
    /// Recent messages or text from conversation
    pub messages: Vec<String>,
    /// Extracted keywords
    pub keywords: Vec<String>,
    /// Current topic (if identified)
    pub topic: Option<String>,
    /// IDs of memories already referenced
    pub referenced_memories: Vec<i64>,
    /// User's apparent intent
    pub intent: Option<String>,
}

impl ConversationContext {
    /// Create from a single message
    pub fn from_message(message: impl Into<String>) -> Self {
        let msg = message.into();
        let keywords = Self::extract_keywords(&msg);
        Self {
            messages: vec![msg],
            keywords,
            topic: None,
            referenced_memories: vec![],
            intent: None,
        }
    }

    /// Create from multiple messages
    pub fn from_messages(messages: Vec<String>) -> Self {
        let all_text = messages.join(" ");
        let keywords = Self::extract_keywords(&all_text);
        Self {
            messages,
            keywords,
            topic: None,
            referenced_memories: vec![],
            intent: None,
        }
    }

    /// Extract keywords from text (simple implementation)
    fn extract_keywords(text: &str) -> Vec<String> {
        // Stop words to filter out
        let stop_words: HashSet<&str> = [
            "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has",
            "had", "do", "does", "did", "will", "would", "could", "should", "may", "might", "can",
            "this", "that", "these", "those", "i", "you", "he", "she", "it", "we", "they", "what",
            "which", "who", "when", "where", "why", "how", "all", "each", "every", "both", "few",
            "more", "most", "other", "some", "such", "no", "nor", "not", "only", "own", "same",
            "so", "than", "too", "very", "just", "and", "but", "or", "if", "because", "as",
            "until", "while", "of", "at", "by", "for", "with", "about", "against", "between",
            "into", "through", "during", "before", "after", "above", "below", "to", "from", "up",
            "down", "in", "out", "on", "off", "over", "under", "again", "further", "then", "once",
            "here", "there", "any", "your", "my", "his", "her", "its", "our", "their", "need",
            "want", "like", "know", "think", "make",
        ]
        .iter()
        .cloned()
        .collect();

        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| word.len() > 2 && !stop_words.contains(word))
            .map(String::from)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect()
    }

    /// Set topic
    pub fn with_topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    /// Add referenced memory IDs
    pub fn with_referenced_memories(mut self, ids: Vec<i64>) -> Self {
        self.referenced_memories = ids;
        self
    }

    /// Set intent
    pub fn with_intent(mut self, intent: impl Into<String>) -> Self {
        self.intent = Some(intent.into());
        self
    }
}

/// Engine for generating memory suggestions
pub struct SuggestionEngine {
    config: SuggestionConfig,
}

impl Default for SuggestionEngine {
    fn default() -> Self {
        Self::new(SuggestionConfig::default())
    }
}

impl SuggestionEngine {
    /// Create a new suggestion engine
    pub fn new(config: SuggestionConfig) -> Self {
        Self { config }
    }

    /// Generate suggestions based on conversation context and available memories
    pub fn generate_suggestions(
        &self,
        context: &ConversationContext,
        memories: &[Memory],
        search_results: Option<&[SearchResult]>,
    ) -> Vec<Suggestion> {
        let mut suggestions = Vec::new();

        // Score each memory
        let mut scored_memories: Vec<(f32, &Memory, SuggestionType, String)> = memories
            .iter()
            .filter(|m| !context.referenced_memories.contains(&m.id))
            .filter_map(|memory| {
                let (score, suggestion_type, reason) =
                    self.score_memory(memory, context, search_results);
                if score >= self.config.min_relevance {
                    Some((score, memory, suggestion_type, reason))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score descending
        scored_memories.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Take top suggestions
        for (score, memory, suggestion_type, reason) in scored_memories
            .into_iter()
            .take(self.config.max_suggestions)
        {
            let keywords: Vec<String> = context
                .keywords
                .iter()
                .filter(|kw| memory.content.to_lowercase().contains(&kw.to_lowercase()))
                .cloned()
                .collect();

            suggestions.push(
                Suggestion::new(Some(memory.clone()), suggestion_type, score, reason)
                    .with_keywords(keywords),
            );
        }

        // Add create suggestion if enabled and context suggests new memory would be useful
        if self.config.enable_create_suggestions {
            if let Some(create_suggestion) = self.suggest_create(context) {
                suggestions.push(create_suggestion);
            }
        }

        suggestions
    }

    /// Score a memory based on context
    fn score_memory(
        &self,
        memory: &Memory,
        context: &ConversationContext,
        search_results: Option<&[SearchResult]>,
    ) -> (f32, SuggestionType, String) {
        let mut total_score = 0.0;
        let mut suggestion_type = SuggestionType::TopicMatch;
        let mut reasons = Vec::new();

        // Keyword matching score
        let keyword_score = self.calculate_keyword_score(memory, context);
        if keyword_score > 0.0 {
            total_score += keyword_score * self.config.keyword_weight;
            reasons.push(format!(
                "matches keywords ({}%)",
                (keyword_score * 100.0) as i32
            ));
        }

        // Semantic similarity score (from search results if available)
        if let Some(results) = search_results {
            if let Some(result) = results.iter().find(|r| r.memory.id == memory.id) {
                let semantic_score = result.match_info.semantic_score.unwrap_or(0.0);
                total_score += semantic_score * self.config.semantic_weight;
                if semantic_score > 0.5 {
                    suggestion_type = SuggestionType::SemanticallySimilar;
                    reasons.push(format!(
                        "semantically similar ({}%)",
                        (semantic_score * 100.0) as i32
                    ));
                }
            }
        }

        // Recency score
        let recency_score = self.calculate_recency_score(memory);
        total_score += recency_score * self.config.recency_weight;
        if recency_score > 0.8 {
            if total_score > 0.5 {
                suggestion_type = SuggestionType::RecentlyAdded;
            }
            reasons.push("recently updated".to_string());
        }

        // Frequency/access score
        let frequency_score = self.calculate_frequency_score(memory);
        total_score += frequency_score * self.config.frequency_weight;
        if frequency_score > 0.7 {
            suggestion_type = SuggestionType::FrequentlyUsed;
            reasons.push("frequently accessed".to_string());
        }

        // Check for potential conflicts
        if self.might_conflict(memory, context) {
            suggestion_type = SuggestionType::PotentialConflict;
            reasons.push("might contain conflicting information".to_string());
        }

        // Check if memory needs review (old and not accessed recently)
        if self.needs_review(memory) {
            suggestion_type = SuggestionType::NeedsReview;
            reasons.push("may need review (outdated)".to_string());
        }

        let reason = if reasons.is_empty() {
            "Related to conversation".to_string()
        } else {
            reasons.join(", ")
        };

        (total_score.min(1.0), suggestion_type, reason)
    }

    /// Calculate keyword matching score
    fn calculate_keyword_score(&self, memory: &Memory, context: &ConversationContext) -> f32 {
        if context.keywords.is_empty() {
            return 0.0;
        }

        let content_lower = memory.content.to_lowercase();
        let tags_lower: Vec<String> = memory.tags.iter().map(|t| t.to_lowercase()).collect();

        let matches: usize = context
            .keywords
            .iter()
            .filter(|kw| {
                let kw_lower = kw.to_lowercase();
                content_lower.contains(&kw_lower)
                    || tags_lower.iter().any(|t| t.contains(&kw_lower))
            })
            .count();

        (matches as f32 / context.keywords.len() as f32).min(1.0)
    }

    /// Calculate recency score
    fn calculate_recency_score(&self, memory: &Memory) -> f32 {
        let age_days = (Utc::now() - memory.updated_at).num_days() as f32;
        let window = self.config.recency_window_days as f32;

        if age_days <= 0.0 {
            1.0
        } else if age_days >= window {
            0.0
        } else {
            1.0 - (age_days / window)
        }
    }

    /// Calculate access frequency score
    fn calculate_frequency_score(&self, memory: &Memory) -> f32 {
        // Normalize access count (assume 100 accesses is high)
        (memory.access_count as f32 / 100.0).min(1.0)
    }

    /// Check if memory might conflict with conversation context
    fn might_conflict(&self, memory: &Memory, context: &ConversationContext) -> bool {
        // Simple heuristic: check for contradiction keywords
        let contradiction_pairs = [
            ("true", "false"),
            ("yes", "no"),
            ("enable", "disable"),
            ("start", "stop"),
            ("add", "remove"),
            ("create", "delete"),
        ];

        let content_lower = memory.content.to_lowercase();
        let context_text = context.messages.join(" ").to_lowercase();

        for (word1, word2) in contradiction_pairs {
            if (content_lower.contains(word1) && context_text.contains(word2))
                || (content_lower.contains(word2) && context_text.contains(word1))
            {
                return true;
            }
        }

        false
    }

    /// Check if memory needs review
    fn needs_review(&self, memory: &Memory) -> bool {
        let age_days = (Utc::now() - memory.updated_at).num_days();
        let last_access_days = memory
            .last_accessed_at
            .map(|dt| (Utc::now() - dt).num_days())
            .unwrap_or(age_days);

        // Needs review if older than 90 days and not accessed in 30 days
        age_days > 90 && last_access_days > 30
    }

    /// Suggest creating a new memory based on context
    fn suggest_create(&self, context: &ConversationContext) -> Option<Suggestion> {
        // Simple heuristic: suggest creation if context mentions decisions, todos, or important facts
        let context_text = context.messages.join(" ").to_lowercase();

        let create_triggers = [
            ("decide", "Decision detected in conversation"),
            ("agreed", "Agreement detected in conversation"),
            ("remember", "User wants to remember something"),
            ("important", "Important information mentioned"),
            ("todo", "Task or todo mentioned"),
            ("deadline", "Deadline mentioned"),
            ("bug", "Bug or issue mentioned"),
            ("fix", "Fix or solution mentioned"),
            ("learn", "Learning opportunity detected"),
        ];

        for (trigger, reason) in create_triggers {
            if context_text.contains(trigger) {
                // Extract a potential content snippet
                let suggested_content = context
                    .messages
                    .last()
                    .cloned()
                    .unwrap_or_else(|| context.keywords.join(" "));

                return Some(
                    Suggestion::new(None, SuggestionType::SuggestCreate, 0.6, reason)
                        .with_suggested_content(suggested_content)
                        .with_keywords(context.keywords.clone()),
                );
            }
        }

        None
    }

    /// Get suggestion configuration
    pub fn config(&self) -> &SuggestionConfig {
        &self.config
    }

    /// Update configuration
    pub fn set_config(&mut self, config: SuggestionConfig) {
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MemoryType, Visibility};
    use std::collections::HashMap;

    fn create_test_memory(id: i64, content: &str, tags: Vec<&str>) -> Memory {
        Memory {
            id,
            content: content.to_string(),
            memory_type: MemoryType::Note,
            tags: tags.into_iter().map(String::from).collect(),
            metadata: HashMap::new(),
            importance: 0.5,
            access_count: 10,
            created_at: Utc::now() - chrono::Duration::days(5),
            updated_at: Utc::now() - chrono::Duration::days(1),
            last_accessed_at: Some(Utc::now() - chrono::Duration::hours(2)),
            owner_id: None,
            visibility: Visibility::Private,
            scope: crate::types::MemoryScope::Global,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None,
        }
    }

    #[test]
    fn test_conversation_context_keyword_extraction() {
        let context =
            ConversationContext::from_message("I need to fix the bug in the authentication system");

        assert!(context.keywords.contains(&"fix".to_string()));
        assert!(context.keywords.contains(&"bug".to_string()));
        assert!(context.keywords.contains(&"authentication".to_string()));
        assert!(context.keywords.contains(&"system".to_string()));
        // Stop words should be filtered
        assert!(!context.keywords.contains(&"the".to_string()));
        assert!(!context.keywords.contains(&"in".to_string()));
    }

    #[test]
    fn test_suggestion_generation() {
        let engine = SuggestionEngine::default();

        let memories = vec![
            create_test_memory(1, "Authentication bug fix for OAuth", vec!["bug", "auth"]),
            create_test_memory(
                2,
                "Database optimization notes",
                vec!["database", "performance"],
            ),
            create_test_memory(3, "OAuth configuration guide", vec!["oauth", "config"]),
        ];

        let context = ConversationContext::from_message("How do I fix the OAuth authentication?");

        let suggestions = engine.generate_suggestions(&context, &memories, None);

        // Should suggest memories related to OAuth and authentication
        assert!(!suggestions.is_empty());

        // First suggestion should be about auth or OAuth
        let first = &suggestions[0];
        assert!(first
            .memory
            .as_ref()
            .map(|m| m.content.to_lowercase().contains("auth")
                || m.content.to_lowercase().contains("oauth"))
            .unwrap_or(false));
    }

    #[test]
    fn test_create_suggestion() {
        let engine = SuggestionEngine::default();
        let memories: Vec<Memory> = vec![];

        let context = ConversationContext::from_message("We decided to use JWT for authentication");

        let suggestions = engine.generate_suggestions(&context, &memories, None);

        // Should suggest creating a memory about the decision
        let create_suggestion = suggestions
            .iter()
            .find(|s| s.suggestion_type == SuggestionType::SuggestCreate);

        assert!(create_suggestion.is_some());
    }

    #[test]
    fn test_keyword_score() {
        let engine = SuggestionEngine::default();

        let memory = create_test_memory(
            1,
            "Rust programming best practices",
            vec!["rust", "programming"],
        );
        let context = ConversationContext::from_message("What are the best practices for Rust?");

        let score = engine.calculate_keyword_score(&memory, &context);
        assert!(score > 0.0);
    }

    #[test]
    fn test_recency_score() {
        let engine = SuggestionEngine::default();

        let mut recent_memory = create_test_memory(1, "Recent note", vec![]);
        recent_memory.updated_at = Utc::now();

        let mut old_memory = create_test_memory(2, "Old note", vec![]);
        old_memory.updated_at = Utc::now() - chrono::Duration::days(60);

        let recent_score = engine.calculate_recency_score(&recent_memory);
        let old_score = engine.calculate_recency_score(&old_memory);

        assert!(recent_score > old_score);
        assert!(recent_score > 0.9);
    }

    #[test]
    fn test_needs_review() {
        let engine = SuggestionEngine::default();

        let mut old_memory = create_test_memory(1, "Old content", vec![]);
        old_memory.updated_at = Utc::now() - chrono::Duration::days(100);
        old_memory.last_accessed_at = Some(Utc::now() - chrono::Duration::days(40));

        assert!(engine.needs_review(&old_memory));

        let mut recent_memory = create_test_memory(2, "Recent content", vec![]);
        recent_memory.updated_at = Utc::now() - chrono::Duration::days(10);

        assert!(!engine.needs_review(&recent_memory));
    }
}
