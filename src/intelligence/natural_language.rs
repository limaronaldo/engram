//! Natural Language Commands (RML-893)
//!
//! Parses natural language input into structured commands.

use crate::types::{EdgeType, MemoryType};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Types of commands that can be parsed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    /// Create a new memory
    Create,
    /// Search for memories
    Search,
    /// Update a memory
    Update,
    /// Delete a memory
    Delete,
    /// Link two memories
    Link,
    /// List memories
    List,
    /// Show statistics
    Stats,
    /// Get help
    Help,
    /// Unknown command
    Unknown,
}

/// A parsed command with extracted parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedCommand {
    /// Type of command
    pub command_type: CommandType,
    /// Main content or query
    pub content: Option<String>,
    /// Target memory ID (for update/delete)
    pub target_id: Option<i64>,
    /// Memory type (for create/search)
    pub memory_type: Option<MemoryType>,
    /// Tags extracted
    pub tags: Vec<String>,
    /// Edge type (for link)
    pub edge_type: Option<EdgeType>,
    /// Related memory ID (for link)
    pub related_id: Option<i64>,
    /// Date/time filter
    pub date_filter: Option<DateFilter>,
    /// Limit for results
    pub limit: Option<i64>,
    /// Original input
    pub original_input: String,
    /// Confidence in parsing (0.0 - 1.0)
    pub confidence: f32,
    /// Additional parameters
    pub params: HashMap<String, String>,
}

/// Date filter for queries
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateFilter {
    pub after: Option<DateTime<Utc>>,
    pub before: Option<DateTime<Utc>>,
}

/// Natural language command parser
pub struct NaturalLanguageParser {
    /// Keywords that indicate create intent
    create_keywords: Vec<&'static str>,
    /// Keywords that indicate search intent
    search_keywords: Vec<&'static str>,
    /// Keywords that indicate delete intent
    delete_keywords: Vec<&'static str>,
    /// Keywords that indicate link intent
    link_keywords: Vec<&'static str>,
    /// Keywords that indicate list intent
    list_keywords: Vec<&'static str>,
}

impl Default for NaturalLanguageParser {
    fn default() -> Self {
        Self::new()
    }
}

impl NaturalLanguageParser {
    /// Create a new parser
    pub fn new() -> Self {
        Self {
            create_keywords: vec![
                "remember",
                "save",
                "store",
                "create",
                "add",
                "note",
                "record",
                "keep",
                "memorize",
                "write down",
                "jot down",
                "make a note",
            ],
            search_keywords: vec![
                "find", "search", "look for", "what", "where", "when", "show me", "get",
                "retrieve", "recall", "fetch", "query", "lookup",
            ],
            delete_keywords: vec!["delete", "remove", "forget", "erase", "discard", "drop"],
            link_keywords: vec!["link", "connect", "relate", "associate", "reference"],
            list_keywords: vec!["list", "show all", "display", "enumerate", "browse"],
        }
    }

    /// Parse a natural language input into a command
    pub fn parse(&self, input: &str) -> ParsedCommand {
        let input_lower = input.to_lowercase();
        let input_trimmed = input.trim();

        // Detect command type
        let (command_type, confidence) = self.detect_command_type(&input_lower);

        // Extract content
        let content = self.extract_content(input_trimmed, &command_type);

        // Extract tags
        let tags = self.extract_tags(&input_lower);

        // Extract memory type
        let memory_type = self.extract_memory_type(&input_lower);

        // Extract IDs
        let (target_id, related_id) = self.extract_ids(&input_lower);

        // Extract edge type
        let edge_type = self.extract_edge_type(&input_lower);

        // Extract date filter
        let date_filter = self.extract_date_filter(&input_lower);

        // Extract limit
        let limit = self.extract_limit(&input_lower);

        ParsedCommand {
            command_type,
            content,
            target_id,
            memory_type,
            tags,
            edge_type,
            related_id,
            date_filter,
            limit,
            original_input: input.to_string(),
            confidence,
            params: HashMap::new(),
        }
    }

    /// Detect the type of command from input
    fn detect_command_type(&self, input: &str) -> (CommandType, f32) {
        // Check for create intent
        for keyword in &self.create_keywords {
            if input.contains(keyword) {
                return (CommandType::Create, 0.9);
            }
        }

        // Check for search intent
        for keyword in &self.search_keywords {
            if input.contains(keyword) {
                return (CommandType::Search, 0.85);
            }
        }

        // Check for delete intent
        for keyword in &self.delete_keywords {
            if input.contains(keyword) {
                return (CommandType::Delete, 0.9);
            }
        }

        // Check for link intent
        for keyword in &self.link_keywords {
            if input.contains(keyword) {
                return (CommandType::Link, 0.85);
            }
        }

        // Check for list intent
        for keyword in &self.list_keywords {
            if input.contains(keyword) {
                return (CommandType::List, 0.85);
            }
        }

        // Check for stats
        if input.contains("stat") || input.contains("count") || input.contains("how many") {
            return (CommandType::Stats, 0.8);
        }

        // Check for help
        if input.contains("help") || input.contains("how to") || input.contains("usage") {
            return (CommandType::Help, 0.9);
        }

        // Default to search if it looks like a question
        if input.ends_with('?') || input.starts_with("what") || input.starts_with("how") {
            return (CommandType::Search, 0.6);
        }

        // Unknown
        (CommandType::Unknown, 0.3)
    }

    /// Extract main content from input
    fn extract_content(&self, input: &str, command_type: &CommandType) -> Option<String> {
        // Remove command keywords to get content
        let patterns_to_remove: &[&str] = match command_type {
            CommandType::Create => &[
                "remember that",
                "remember:",
                "save:",
                "note:",
                "add:",
                "create:",
                "remember",
                "save",
                "note",
                "add",
                "create",
                "please",
                "can you",
            ],
            CommandType::Search => &[
                "find",
                "search for",
                "search",
                "look for",
                "show me",
                "get",
                "what is",
                "what are",
                "where is",
                "when did",
                "please",
                "can you",
            ],
            CommandType::Delete => &["delete", "remove", "forget", "erase", "please", "can you"],
            _ => &["please", "can you"],
        };

        let mut content = input.to_string();
        for pattern in patterns_to_remove {
            content = content.replace(pattern, "");
            // Also try with capital first letter
            let capitalized = pattern
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string() + &pattern[1..])
                .unwrap_or_default();
            content = content.replace(&capitalized, "");
        }

        let content = content.trim().to_string();
        if content.is_empty() {
            None
        } else {
            Some(content)
        }
    }

    /// Extract tags from input
    fn extract_tags(&self, input: &str) -> Vec<String> {
        let mut tags = Vec::new();

        // Look for #hashtags
        for word in input.split_whitespace() {
            if word.starts_with('#') {
                let tag = word
                    .trim_start_matches('#')
                    .trim_matches(|c: char| !c.is_alphanumeric());
                if !tag.is_empty() {
                    tags.push(tag.to_string());
                }
            }
        }

        // Look for "tag:" or "tags:" pattern
        if let Some(pos) = input.find("tag:") {
            let rest = &input[pos + 4..];
            for word in rest.split_whitespace() {
                if word.chars().all(|c| c.is_alphanumeric() || c == ',') {
                    for tag in word.split(',') {
                        let tag = tag.trim();
                        if !tag.is_empty() {
                            tags.push(tag.to_string());
                        }
                    }
                    break;
                }
            }
        }

        tags
    }

    /// Extract memory type from input
    fn extract_memory_type(&self, input: &str) -> Option<MemoryType> {
        if input.contains("todo") || input.contains("task") {
            Some(MemoryType::Todo)
        } else if input.contains("decision") || input.contains("decided") {
            Some(MemoryType::Decision)
        } else if input.contains("issue") || input.contains("bug") || input.contains("problem") {
            Some(MemoryType::Issue)
        } else if input.contains("preference") || input.contains("prefer") {
            Some(MemoryType::Preference)
        } else if input.contains("learn") || input.contains("til") {
            Some(MemoryType::Learning)
        } else if input.contains("context") || input.contains("background") {
            Some(MemoryType::Context)
        } else {
            None
        }
    }

    /// Extract memory IDs from input
    fn extract_ids(&self, input: &str) -> (Option<i64>, Option<i64>) {
        let mut ids: Vec<i64> = Vec::new();

        // Look for patterns like "memory 123", "#123", "id 123", or "id:123"
        let patterns = ["memory ", "id ", "id:", "#"];

        for pattern in patterns {
            if let Some(pos) = input.find(pattern) {
                let rest = &input[pos + pattern.len()..];
                let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(id) = num_str.parse::<i64>() {
                    ids.push(id);
                }
            }
        }

        // Also look for standalone numbers that might be IDs
        for word in input.split_whitespace() {
            if let Ok(id) = word.parse::<i64>() {
                if id > 0 && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }

        match ids.len() {
            0 => (None, None),
            1 => (Some(ids[0]), None),
            _ => (Some(ids[0]), Some(ids[1])),
        }
    }

    /// Extract edge type from input
    fn extract_edge_type(&self, input: &str) -> Option<EdgeType> {
        if input.contains("supersede") || input.contains("replace") {
            Some(EdgeType::Supersedes)
        } else if input.contains("contradict") || input.contains("conflict") {
            Some(EdgeType::Contradicts)
        } else if input.contains("implement") {
            Some(EdgeType::Implements)
        } else if input.contains("extend") {
            Some(EdgeType::Extends)
        } else if input.contains("reference") || input.contains("refer") {
            Some(EdgeType::References)
        } else if input.contains("depend") || input.contains("require") {
            Some(EdgeType::DependsOn)
        } else if input.contains("block") {
            Some(EdgeType::Blocks)
        } else if input.contains("follow") {
            Some(EdgeType::FollowsUp)
        } else if input.contains("relate") || input.contains("link") {
            Some(EdgeType::RelatedTo)
        } else {
            None
        }
    }

    /// Extract date filter from input
    fn extract_date_filter(&self, input: &str) -> Option<DateFilter> {
        let mut after = None;
        let mut before = None;

        // Look for "last X days/weeks"
        if input.contains("last") {
            if let Some(days) = self.extract_duration_days(input) {
                after = Some(Utc::now() - chrono::Duration::days(days));
            }
        }

        // Look for "today", "yesterday", "this week"
        if input.contains("today") {
            let today = Utc::now().date_naive();
            after = Some(today.and_hms_opt(0, 0, 0).unwrap().and_utc());
        } else if input.contains("yesterday") {
            let yesterday = Utc::now().date_naive() - chrono::Duration::days(1);
            after = Some(yesterday.and_hms_opt(0, 0, 0).unwrap().and_utc());
            before = Some(
                Utc::now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc(),
            );
        } else if input.contains("this week") {
            after = Some(Utc::now() - chrono::Duration::days(7));
        } else if input.contains("this month") {
            after = Some(Utc::now() - chrono::Duration::days(30));
        }

        if after.is_some() || before.is_some() {
            Some(DateFilter { after, before })
        } else {
            None
        }
    }

    /// Extract duration in days from phrases like "last 7 days"
    fn extract_duration_days(&self, input: &str) -> Option<i64> {
        // Look for patterns like "last 7 days", "last week", "last month"
        for word in input.split_whitespace() {
            if let Ok(num) = word.parse::<i64>() {
                if input.contains("day") {
                    return Some(num);
                } else if input.contains("week") {
                    return Some(num * 7);
                } else if input.contains("month") {
                    return Some(num * 30);
                }
            }
        }

        // Handle special cases
        if input.contains("last week") {
            Some(7)
        } else if input.contains("last month") {
            Some(30)
        } else if input.contains("last year") {
            Some(365)
        } else {
            None
        }
    }

    /// Extract result limit from input
    fn extract_limit(&self, input: &str) -> Option<i64> {
        // Look for patterns like "top 10", "first 5", "limit 20"
        let patterns = ["top ", "first ", "limit "];

        for pattern in patterns {
            if let Some(pos) = input.find(pattern) {
                let rest = &input[pos + pattern.len()..];
                let num_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(limit) = num_str.parse::<i64>() {
                    return Some(limit);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_create() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("Remember that the API key is abc123");
        assert_eq!(cmd.command_type, CommandType::Create);
        assert!(cmd.content.is_some());
        assert!(cmd.confidence > 0.8);
    }

    #[test]
    fn test_detect_search() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("Find all memories about authentication");
        assert_eq!(cmd.command_type, CommandType::Search);
        assert!(cmd.content.unwrap().contains("authentication"));
    }

    #[test]
    fn test_extract_tags() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("Save this note #important #work");
        assert!(cmd.tags.contains(&"important".to_string()));
        assert!(cmd.tags.contains(&"work".to_string()));
    }

    #[test]
    fn test_extract_memory_type() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("Add a todo: fix the bug");
        assert_eq!(cmd.memory_type, Some(MemoryType::Todo));

        let cmd = parser.parse("Record this decision: use JWT");
        assert_eq!(cmd.memory_type, Some(MemoryType::Decision));
    }

    #[test]
    fn test_extract_ids() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("Link memory 123 to memory 456");
        assert_eq!(cmd.target_id, Some(123));
        assert_eq!(cmd.related_id, Some(456));
    }

    #[test]
    fn test_extract_date_filter() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("Find memories from last week");
        assert!(cmd.date_filter.is_some());
        assert!(cmd.date_filter.unwrap().after.is_some());
    }

    #[test]
    fn test_extract_limit() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("Show top 10 recent memories");
        assert_eq!(cmd.limit, Some(10));
    }

    #[test]
    fn test_question_as_search() {
        let parser = NaturalLanguageParser::new();

        let cmd = parser.parse("What is the database password?");
        assert_eq!(cmd.command_type, CommandType::Search);
    }
}
