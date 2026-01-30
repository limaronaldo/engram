//! Auto-Capture Mode for Proactive Memory (RML-903)
//!
//! Automatically detects and captures valuable information from conversations:
//! - Key decisions and their rationale
//! - Action items and todos
//! - Important facts and context
//! - User preferences and patterns
//! - Technical learnings and insights

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::types::{Memory, MemoryType};

/// Configuration for auto-capture behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoCaptureConfig {
    /// Enable auto-capture mode
    pub enabled: bool,
    /// Minimum confidence threshold for capture (0.0 - 1.0)
    pub min_confidence: f32,
    /// Types of content to capture
    pub capture_types: HashSet<CaptureType>,
    /// Maximum captures per conversation turn
    pub max_per_turn: usize,
    /// Require user confirmation before saving
    pub require_confirmation: bool,
    /// Keywords that trigger capture consideration
    pub trigger_keywords: Vec<String>,
    /// Patterns to ignore (e.g., greetings, small talk)
    pub ignore_patterns: Vec<String>,
}

impl Default for AutoCaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_confidence: 0.6,
            capture_types: vec![
                CaptureType::Decision,
                CaptureType::ActionItem,
                CaptureType::KeyFact,
                CaptureType::Preference,
                CaptureType::Learning,
            ]
            .into_iter()
            .collect(),
            max_per_turn: 3,
            require_confirmation: true,
            trigger_keywords: vec![
                "decide".to_string(),
                "decided".to_string(),
                "decision".to_string(),
                "todo".to_string(),
                "remember".to_string(),
                "important".to_string(),
                "always".to_string(),
                "never".to_string(),
                "prefer".to_string(),
                "learned".to_string(),
                "note".to_string(),
                "key".to_string(),
                "critical".to_string(),
                "must".to_string(),
                "should".to_string(),
            ],
            ignore_patterns: vec![
                "hello".to_string(),
                "hi".to_string(),
                "thanks".to_string(),
                "thank you".to_string(),
                "bye".to_string(),
                "goodbye".to_string(),
                "ok".to_string(),
                "okay".to_string(),
                "sure".to_string(),
                "yes".to_string(),
                "no".to_string(),
            ],
        }
    }
}

/// Types of content that can be auto-captured
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureType {
    /// A decision made during conversation
    Decision,
    /// An action item or task to do
    ActionItem,
    /// An important fact or piece of context
    KeyFact,
    /// A user preference or pattern
    Preference,
    /// A technical learning or insight
    Learning,
    /// A question for follow-up
    Question,
    /// An issue or problem identified
    Issue,
    /// A code snippet or technical artifact
    CodeSnippet,
}

impl CaptureType {
    /// Convert to MemoryType for storage
    pub fn to_memory_type(&self) -> MemoryType {
        match self {
            CaptureType::Decision => MemoryType::Decision,
            CaptureType::ActionItem => MemoryType::Todo,
            CaptureType::KeyFact => MemoryType::Note,
            CaptureType::Preference => MemoryType::Preference,
            CaptureType::Learning => MemoryType::Learning,
            CaptureType::Question => MemoryType::Note,
            CaptureType::Issue => MemoryType::Issue,
            CaptureType::CodeSnippet => MemoryType::Note,
        }
    }

    /// Get detection patterns for this type
    fn patterns(&self) -> Vec<&'static str> {
        match self {
            CaptureType::Decision => vec![
                "decided to",
                "decision is",
                "we'll go with",
                "let's use",
                "the approach is",
                "we chose",
                "going forward",
                "from now on",
            ],
            CaptureType::ActionItem => vec![
                "todo:",
                "action item:",
                "need to",
                "should do",
                "will do",
                "must do",
                "task:",
                "follow up",
                "remember to",
            ],
            CaptureType::KeyFact => vec![
                "important:",
                "note:",
                "key point",
                "the fact is",
                "actually,",
                "turns out",
                "discovered that",
                "found that",
            ],
            CaptureType::Preference => vec![
                "prefer",
                "like to",
                "always use",
                "never use",
                "my style",
                "i want",
                "i don't want",
                "please always",
                "please never",
            ],
            CaptureType::Learning => vec![
                "learned that",
                "til:",
                "today i learned",
                "insight:",
                "realization:",
                "now i understand",
                "turns out that",
            ],
            CaptureType::Question => vec![
                "question:",
                "need to find out",
                "investigate",
                "look into",
                "figure out",
                "unclear about",
            ],
            CaptureType::Issue => vec![
                "bug:",
                "issue:",
                "problem:",
                "error:",
                "broken:",
                "doesn't work",
                "failing",
            ],
            CaptureType::CodeSnippet => vec![
                "```", "code:", "snippet:", "function", "class", "const", "let", "fn ",
            ],
        }
    }
}

/// A candidate for auto-capture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCandidate {
    /// The content to potentially capture
    pub content: String,
    /// Detected type
    pub capture_type: CaptureType,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f32,
    /// Source context (where it came from)
    pub source: String,
    /// Suggested tags
    pub suggested_tags: Vec<String>,
    /// Suggested importance (0.0 - 1.0)
    pub suggested_importance: f32,
    /// Detection timestamp
    pub detected_at: DateTime<Utc>,
    /// Reason for capture
    pub reason: String,
}

impl CaptureCandidate {
    /// Convert to a Memory for storage
    pub fn to_memory(&self) -> Memory {
        Memory {
            id: 0, // Will be assigned by storage
            content: self.content.clone(),
            memory_type: self.capture_type.to_memory_type(),
            tags: self.suggested_tags.clone(),
            metadata: std::collections::HashMap::new(),
            importance: self.suggested_importance,
            access_count: 0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            last_accessed_at: None,
            owner_id: None,
            visibility: crate::types::Visibility::Private,
            scope: crate::types::MemoryScope::Global,
            workspace: "default".to_string(),
            tier: crate::types::MemoryTier::Permanent,
            version: 1,
            has_embedding: false,
            expires_at: None,
            content_hash: None, // Will be computed on storage
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            procedure_success_count: 0,
            procedure_failure_count: 0,
            summary_of_id: None,
            lifecycle_state: crate::types::LifecycleState::Active,
        }
    }
}

/// Auto-capture engine
pub struct AutoCaptureEngine {
    config: AutoCaptureConfig,
}

impl AutoCaptureEngine {
    pub fn new(config: AutoCaptureConfig) -> Self {
        Self { config }
    }

    pub fn with_default_config() -> Self {
        Self::new(AutoCaptureConfig::default())
    }

    /// Analyze text and detect potential captures
    pub fn analyze(&self, text: &str, source: &str) -> Vec<CaptureCandidate> {
        if !self.config.enabled {
            return Vec::new();
        }

        // Skip if matches ignore patterns
        let text_lower = text.to_lowercase();
        if self.should_ignore(&text_lower) {
            return Vec::new();
        }

        let mut candidates = Vec::new();

        // Check each capture type
        for capture_type in &self.config.capture_types {
            if let Some(candidate) = self.detect_type(text, &text_lower, *capture_type, source) {
                if candidate.confidence >= self.config.min_confidence {
                    candidates.push(candidate);
                }
            }
        }

        // Sort by confidence and limit
        candidates.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
        candidates.truncate(self.config.max_per_turn);

        candidates
    }

    /// Check if text should be ignored
    fn should_ignore(&self, text_lower: &str) -> bool {
        // Too short
        if text_lower.len() < 10 {
            return true;
        }

        // Matches ignore patterns
        for pattern in &self.config.ignore_patterns {
            if text_lower.trim() == pattern.as_str() {
                return true;
            }
        }

        false
    }

    /// Detect a specific capture type
    fn detect_type(
        &self,
        text: &str,
        text_lower: &str,
        capture_type: CaptureType,
        source: &str,
    ) -> Option<CaptureCandidate> {
        let patterns = capture_type.patterns();
        let mut confidence: f32 = 0.0;
        let mut matched_pattern = "";

        // Check patterns
        for pattern in patterns {
            if text_lower.contains(pattern) {
                confidence = 0.7;
                matched_pattern = pattern;
                break;
            }
        }

        // Boost confidence for trigger keywords
        let trigger_count = self
            .config
            .trigger_keywords
            .iter()
            .filter(|kw| text_lower.contains(kw.as_str()))
            .count();
        confidence += (trigger_count as f32 * 0.05).min(0.2);

        // Boost for explicit markers
        if text_lower.contains("remember:") || text_lower.contains("important:") {
            confidence += 0.15;
        }

        // Minimum threshold check
        if confidence < 0.3 {
            return None;
        }

        // Extract the relevant content
        let content = self.extract_content(text, capture_type);
        if content.is_empty() {
            return None;
        }

        // Suggest tags based on content
        let suggested_tags = self.suggest_tags(&content, capture_type);

        // Calculate importance
        let suggested_importance = self.calculate_importance(&content, capture_type, confidence);

        Some(CaptureCandidate {
            content,
            capture_type,
            confidence: confidence.min(1.0),
            source: source.to_string(),
            suggested_tags,
            suggested_importance,
            detected_at: Utc::now(),
            reason: format!("Matched pattern: '{}'", matched_pattern),
        })
    }

    /// Extract the relevant content for capture
    fn extract_content(&self, text: &str, capture_type: CaptureType) -> String {
        let text_lower = text.to_lowercase();

        // Try to extract after common markers
        let markers = match capture_type {
            CaptureType::Decision => vec!["decided to", "decision:", "we'll"],
            CaptureType::ActionItem => vec!["todo:", "action:", "need to"],
            CaptureType::KeyFact => vec!["important:", "note:", "key:"],
            CaptureType::Preference => vec!["prefer", "always", "never"],
            CaptureType::Learning => vec!["learned", "til:", "insight:"],
            CaptureType::Question => vec!["question:", "investigate"],
            CaptureType::Issue => vec!["bug:", "issue:", "problem:"],
            CaptureType::CodeSnippet => vec!["```", "code:"],
        };

        for marker in markers {
            if let Some(pos) = text_lower.find(marker) {
                let start = pos + marker.len();
                let extracted = text[start..].trim();
                // Take until end of sentence or paragraph
                let end = extracted
                    .find(|c: char| c == '\n' || c == '.' && extracted.len() > 10)
                    .unwrap_or(extracted.len().min(500));
                return extracted[..end].trim().to_string();
            }
        }

        // If no marker found, use the whole text (truncated)
        let max_len = 500;
        if text.len() <= max_len {
            text.trim().to_string()
        } else {
            format!("{}...", &text[..max_len].trim())
        }
    }

    /// Suggest tags based on content
    fn suggest_tags(&self, content: &str, capture_type: CaptureType) -> Vec<String> {
        let mut tags = Vec::new();
        let content_lower = content.to_lowercase();

        // Add type-based tag
        tags.push(format!("auto-{:?}", capture_type).to_lowercase());

        // Common technology tags
        let tech_tags = [
            ("rust", "rust"),
            ("python", "python"),
            ("javascript", "javascript"),
            ("typescript", "typescript"),
            ("react", "react"),
            ("sql", "sql"),
            ("api", "api"),
            ("database", "database"),
            ("frontend", "frontend"),
            ("backend", "backend"),
        ];

        for (keyword, tag) in tech_tags {
            if content_lower.contains(keyword) {
                tags.push(tag.to_string());
            }
        }

        // Domain tags
        let domain_tags = [
            ("auth", "authentication"),
            ("login", "authentication"),
            ("security", "security"),
            ("performance", "performance"),
            ("test", "testing"),
            ("deploy", "deployment"),
            ("config", "configuration"),
            ("error", "error-handling"),
        ];

        for (keyword, tag) in domain_tags {
            if content_lower.contains(keyword) {
                tags.push(tag.to_string());
            }
        }

        // Deduplicate
        tags.sort();
        tags.dedup();
        tags.truncate(5);

        tags
    }

    /// Calculate suggested importance
    fn calculate_importance(
        &self,
        content: &str,
        capture_type: CaptureType,
        confidence: f32,
    ) -> f32 {
        let content_lower = content.to_lowercase();
        let mut importance: f32 = 0.5;

        // Base importance by type
        importance += match capture_type {
            CaptureType::Decision => 0.2,
            CaptureType::ActionItem => 0.15,
            CaptureType::Issue => 0.15,
            CaptureType::Preference => 0.1,
            CaptureType::Learning => 0.1,
            CaptureType::KeyFact => 0.1,
            CaptureType::Question => 0.05,
            CaptureType::CodeSnippet => 0.05,
        };

        // Boost for urgency indicators
        let urgency_words = ["critical", "urgent", "asap", "immediately", "blocker"];
        for word in urgency_words {
            if content_lower.contains(word) {
                importance += 0.1;
            }
        }

        // Boost based on confidence
        importance += confidence * 0.1;

        importance.min(1.0)
    }

    /// Update configuration
    pub fn set_config(&mut self, config: AutoCaptureConfig) {
        self.config = config;
    }

    /// Enable/disable auto-capture
    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    /// Get current config
    pub fn config(&self) -> &AutoCaptureConfig {
        &self.config
    }
}

/// Conversation context for multi-turn capture
#[derive(Debug, Default)]
pub struct ConversationTracker {
    /// Recent messages in the conversation
    messages: Vec<TrackedMessage>,
    /// Candidates detected but not yet confirmed
    pending_captures: Vec<CaptureCandidate>,
    /// Maximum messages to track
    max_messages: usize,
}

#[derive(Debug, Clone)]
struct TrackedMessage {
    content: String,
    role: String,
    #[allow(dead_code)]
    timestamp: DateTime<Utc>,
}

impl ConversationTracker {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::new(),
            pending_captures: Vec::new(),
            max_messages,
        }
    }

    /// Add a message to the tracker
    pub fn add_message(&mut self, content: &str, role: &str) {
        self.messages.push(TrackedMessage {
            content: content.to_string(),
            role: role.to_string(),
            timestamp: Utc::now(),
        });

        // Trim old messages
        if self.messages.len() > self.max_messages {
            self.messages.remove(0);
        }
    }

    /// Get recent context as a string
    pub fn recent_context(&self, num_messages: usize) -> String {
        self.messages
            .iter()
            .rev()
            .take(num_messages)
            .rev()
            .map(|m| format!("[{}]: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Add pending capture
    pub fn add_pending(&mut self, candidate: CaptureCandidate) {
        self.pending_captures.push(candidate);
    }

    /// Get pending captures
    pub fn pending(&self) -> &[CaptureCandidate] {
        &self.pending_captures
    }

    /// Clear pending captures
    pub fn clear_pending(&mut self) {
        self.pending_captures.clear();
    }

    /// Confirm and remove a pending capture
    pub fn confirm_pending(&mut self, index: usize) -> Option<CaptureCandidate> {
        if index < self.pending_captures.len() {
            Some(self.pending_captures.remove(index))
        } else {
            None
        }
    }

    /// Reject a pending capture
    pub fn reject_pending(&mut self, index: usize) {
        if index < self.pending_captures.len() {
            self.pending_captures.remove(index);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_capture_decision() {
        let engine = AutoCaptureEngine::with_default_config();
        let candidates = engine.analyze(
            "We decided to use Rust for the backend because of performance",
            "conversation",
        );

        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].capture_type, CaptureType::Decision);
        assert!(candidates[0].confidence >= 0.6);
    }

    #[test]
    fn test_auto_capture_action_item() {
        let engine = AutoCaptureEngine::with_default_config();
        let candidates = engine.analyze(
            "TODO: implement the authentication module before Friday",
            "conversation",
        );

        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].capture_type, CaptureType::ActionItem);
    }

    #[test]
    fn test_auto_capture_preference() {
        let engine = AutoCaptureEngine::with_default_config();
        let candidates = engine.analyze(
            "I always prefer using TypeScript over JavaScript for better type safety",
            "conversation",
        );

        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].capture_type, CaptureType::Preference);
    }

    #[test]
    fn test_auto_capture_learning() {
        let engine = AutoCaptureEngine::with_default_config();
        let candidates = engine.analyze(
            "TIL: Rust's ownership system prevents data races at compile time",
            "conversation",
        );

        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].capture_type, CaptureType::Learning);
    }

    #[test]
    fn test_ignore_short_text() {
        let engine = AutoCaptureEngine::with_default_config();
        let candidates = engine.analyze("ok", "conversation");
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_ignore_greetings() {
        let engine = AutoCaptureEngine::with_default_config();
        let candidates = engine.analyze("hello", "conversation");
        assert!(candidates.is_empty());
    }

    #[test]
    fn test_suggest_tags() {
        let engine = AutoCaptureEngine::with_default_config();
        let tags = engine.suggest_tags(
            "implement rust api for authentication",
            CaptureType::ActionItem,
        );

        assert!(tags.contains(&"rust".to_string()));
        assert!(tags.contains(&"api".to_string()));
        assert!(tags.contains(&"authentication".to_string()));
    }

    #[test]
    fn test_conversation_tracker() {
        let mut tracker = ConversationTracker::new(10);

        tracker.add_message("Hello", "user");
        tracker.add_message("Hi there!", "assistant");
        tracker.add_message("I need help with Rust", "user");

        let context = tracker.recent_context(2);
        assert!(context.contains("Hi there!"));
        assert!(context.contains("I need help with Rust"));
    }

    #[test]
    fn test_pending_captures() {
        let mut tracker = ConversationTracker::new(10);

        let candidate = CaptureCandidate {
            content: "Use async/await".to_string(),
            capture_type: CaptureType::Decision,
            confidence: 0.8,
            source: "test".to_string(),
            suggested_tags: vec!["rust".to_string()],
            suggested_importance: 0.7,
            detected_at: Utc::now(),
            reason: "test".to_string(),
        };

        tracker.add_pending(candidate);
        assert_eq!(tracker.pending().len(), 1);

        let confirmed = tracker.confirm_pending(0);
        assert!(confirmed.is_some());
        assert_eq!(tracker.pending().len(), 0);
    }

    #[test]
    fn test_capture_to_memory() {
        let candidate = CaptureCandidate {
            content: "Always use Rust for performance-critical code".to_string(),
            capture_type: CaptureType::Preference,
            confidence: 0.85,
            source: "conversation".to_string(),
            suggested_tags: vec!["rust".to_string(), "performance".to_string()],
            suggested_importance: 0.8,
            detected_at: Utc::now(),
            reason: "Matched pattern".to_string(),
        };

        let memory = candidate.to_memory();
        assert_eq!(memory.content, candidate.content);
        assert_eq!(memory.memory_type, MemoryType::Preference);
        assert_eq!(memory.tags, candidate.suggested_tags);
    }

    #[test]
    fn test_disabled_capture() {
        let config = AutoCaptureConfig {
            enabled: false,
            ..Default::default()
        };

        let engine = AutoCaptureEngine::new(config);
        let candidates = engine.analyze("We decided to use Rust for everything", "conversation");

        assert!(candidates.is_empty());
    }
}
