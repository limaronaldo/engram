//! Content utilities for memory display and manipulation
//!
//! Provides:
//! - **Soft trim**: Smart truncation preserving head (60%) and tail (30%) with ellipsis
//! - **Compact preview**: Short preview for list views
//! - **Content statistics**: Character/word/line counts

use serde::{Deserialize, Serialize};

/// Configuration for soft trim operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftTrimConfig {
    /// Maximum total characters (default: 500)
    pub max_chars: usize,
    /// Percentage of max_chars for head portion (default: 60)
    pub head_percent: usize,
    /// Percentage of max_chars for tail portion (default: 30)
    pub tail_percent: usize,
    /// Ellipsis string to use (default: "\n...\n")
    pub ellipsis: String,
    /// Preserve word boundaries (default: true)
    pub preserve_words: bool,
}

impl Default for SoftTrimConfig {
    fn default() -> Self {
        Self {
            max_chars: 500,
            head_percent: 60,
            tail_percent: 30,
            ellipsis: "\n...\n".to_string(),
            preserve_words: true,
        }
    }
}

/// Result of soft trim operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoftTrimResult {
    /// The trimmed content
    pub content: String,
    /// Whether the content was actually trimmed
    pub was_trimmed: bool,
    /// Original character count
    pub original_chars: usize,
    /// Trimmed character count
    pub trimmed_chars: usize,
    /// Characters removed
    pub chars_removed: usize,
}

/// Compact memory representation for list views
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactMemory {
    /// Memory ID
    pub id: i64,
    /// First line or preview of content (max 100 chars)
    pub preview: String,
    /// Memory type
    pub memory_type: String,
    /// Tags
    pub tags: Vec<String>,
    /// Importance score
    pub importance: Option<f32>,
    /// Created timestamp
    pub created_at: String,
    /// Updated timestamp
    pub updated_at: String,
    /// Workspace
    pub workspace: String,
    /// Tier (permanent/daily)
    pub tier: String,
    /// Full content character count
    pub content_length: usize,
    /// Whether content was truncated for preview
    pub is_truncated: bool,
}

/// Content statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentStats {
    /// Character count
    pub chars: usize,
    /// Word count (whitespace-separated)
    pub words: usize,
    /// Line count
    pub lines: usize,
    /// Sentence count (approximate)
    pub sentences: usize,
    /// Paragraph count (double newline separated)
    pub paragraphs: usize,
}

/// Perform soft trim on content
///
/// Preserves the beginning (head) and end (tail) of content while
/// removing the middle portion if content exceeds max_chars.
///
/// Default split: 60% head, 30% tail, 10% for ellipsis overhead
///
/// # Example
///
/// ```
/// use engram::intelligence::content_utils::{soft_trim, SoftTrimConfig};
///
/// let long_content = "A".repeat(1000);
/// let result = soft_trim(&long_content, &SoftTrimConfig::default());
/// assert!(result.was_trimmed);
/// assert!(result.content.len() < 1000);
/// assert!(result.content.contains("..."));
/// ```
pub fn soft_trim(content: &str, config: &SoftTrimConfig) -> SoftTrimResult {
    let original_chars = content.chars().count();

    // If content fits, return as-is
    if original_chars <= config.max_chars {
        return SoftTrimResult {
            content: content.to_string(),
            was_trimmed: false,
            original_chars,
            trimmed_chars: original_chars,
            chars_removed: 0,
        };
    }

    // Calculate head and tail sizes (in characters, not bytes)
    let ellipsis_char_len = config.ellipsis.chars().count();
    let available = config.max_chars.saturating_sub(ellipsis_char_len);
    let head_char_count = (available * config.head_percent) / 100;
    let tail_char_count = (available * config.tail_percent) / 100;

    // Convert character count to byte index for head
    let head_byte_end: usize = content
        .char_indices()
        .take(head_char_count)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);

    let mut head_end = head_byte_end;
    if config.preserve_words && head_end < content.len() {
        // Find last space before head_end
        if let Some(last_space) = content[..head_end].rfind(|c: char| c.is_whitespace()) {
            if last_space > head_end / 2 {
                head_end = last_space;
            }
        }
    }

    // Convert character count from end to byte index for tail
    let total_chars = original_chars;
    let tail_start_char = total_chars.saturating_sub(tail_char_count);
    let tail_byte_start: usize = content
        .char_indices()
        .nth(tail_start_char)
        .map(|(i, _)| i)
        .unwrap_or(content.len());

    let mut tail_start = tail_byte_start;
    if config.preserve_words && tail_start > 0 && tail_start < content.len() {
        // Find first space after tail_start
        if let Some(first_space) = content[tail_start..].find(|c: char| c.is_whitespace()) {
            let new_start = tail_start + first_space + 1;
            if new_start < content.len() {
                tail_start = new_start;
            }
        }
    }

    // Ensure head and tail don't overlap
    if head_end >= tail_start {
        // Content is borderline - just truncate end
        let truncate_byte_end: usize = content
            .char_indices()
            .take(config.max_chars)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(content.len());
        let truncated = &content[..truncate_byte_end.min(content.len())];
        let trimmed_chars = truncated.chars().count() + ellipsis_char_len;
        return SoftTrimResult {
            content: format!("{}{}", truncated.trim_end(), config.ellipsis.trim()),
            was_trimmed: true,
            original_chars,
            trimmed_chars,
            chars_removed: original_chars - truncated.chars().count(),
        };
    }

    let head = content[..head_end].trim_end();
    let tail = content[tail_start..].trim_start();
    let trimmed = format!("{}{}{}", head, config.ellipsis, tail);

    SoftTrimResult {
        content: trimmed.clone(),
        was_trimmed: true,
        original_chars,
        trimmed_chars: trimmed.chars().count(),
        chars_removed: original_chars - head.chars().count() - tail.chars().count(),
    }
}

/// Generate a compact preview of content
///
/// Returns the first line or first N characters, whichever is shorter.
pub fn compact_preview(content: &str, max_chars: usize) -> (String, bool) {
    let content = content.trim();

    if content.is_empty() {
        return (String::new(), false);
    }

    // Get first line
    let first_line = content.lines().next().unwrap_or(content);

    // Use character count for comparison (not byte length)
    let char_count = first_line.chars().count();
    if char_count <= max_chars {
        let is_truncated = content.len() > first_line.len();
        return (first_line.to_string(), is_truncated);
    }

    // Find byte position of max_chars'th character (UTF-8 safe)
    let mut byte_end = first_line
        .char_indices()
        .nth(max_chars.min(char_count))
        .map(|(pos, _)| pos)
        .unwrap_or(first_line.len());

    // Truncate at word boundary if possible
    let slice_to_check = &first_line[..byte_end];
    if let Some(last_space) = slice_to_check.rfind(' ') {
        // Only use space if it's in the latter half
        if last_space > byte_end / 2 {
            byte_end = last_space;
        }
    }

    let preview = format!("{}...", first_line[..byte_end].trim_end());
    (preview, true)
}

/// Calculate content statistics
pub fn content_stats(content: &str) -> ContentStats {
    let chars = content.chars().count(); // Use actual character count, not byte length
    let words = content.split_whitespace().count();
    let lines = content.lines().count().max(1);

    // Approximate sentence count (ends with . ! ?)
    let sentences = content
        .chars()
        .filter(|c| *c == '.' || *c == '!' || *c == '?')
        .count()
        .max(1);

    // Paragraph count (separated by blank lines)
    let paragraphs = content
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .count()
        .max(1);

    ContentStats {
        chars,
        words,
        lines,
        sentences,
        paragraphs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_soft_trim_short_content() {
        let content = "Short content";
        let result = soft_trim(content, &SoftTrimConfig::default());

        assert!(!result.was_trimmed);
        assert_eq!(result.content, content);
        assert_eq!(result.chars_removed, 0);
    }

    #[test]
    fn test_soft_trim_long_content() {
        let content = "A".repeat(1000);
        let config = SoftTrimConfig {
            max_chars: 100,
            ..Default::default()
        };
        let result = soft_trim(&content, &config);

        assert!(result.was_trimmed);
        assert!(result.content.len() <= 100);
        assert!(result.content.contains("..."));
        assert!(result.chars_removed > 0);
    }

    #[test]
    fn test_soft_trim_preserves_head_and_tail() {
        let content = format!(
            "HEADER: Important beginning content. {} FOOTER: Critical ending info.",
            "Middle content that can be removed. ".repeat(50)
        );
        let config = SoftTrimConfig {
            max_chars: 200,
            ..Default::default()
        };
        let result = soft_trim(&content, &config);

        assert!(result.was_trimmed);
        assert!(result.content.starts_with("HEADER"));
        assert!(result.content.ends_with("info."));
    }

    #[test]
    fn test_soft_trim_word_boundaries() {
        let content = "The quick brown fox jumps over the lazy dog. ".repeat(20);
        let config = SoftTrimConfig {
            max_chars: 100,
            preserve_words: true,
            ..Default::default()
        };
        let result = soft_trim(&content, &config);

        // Should not break in middle of a word
        assert!(!result.content.ends_with("Th"));
        assert!(!result.content.ends_with("fo"));
    }

    #[test]
    fn test_compact_preview_short() {
        let content = "Short content";
        let (preview, truncated) = compact_preview(content, 100);

        assert_eq!(preview, "Short content");
        assert!(!truncated);
    }

    #[test]
    fn test_compact_preview_long() {
        let content = "This is a very long first line that exceeds the maximum character limit for preview display";
        let (preview, truncated) = compact_preview(content, 30);

        assert!(preview.len() <= 33); // 30 + "..."
        assert!(preview.ends_with("..."));
        assert!(truncated);
    }

    #[test]
    fn test_compact_preview_multiline() {
        let content = "First line only\nSecond line ignored\nThird line also";
        let (preview, truncated) = compact_preview(content, 100);

        assert_eq!(preview, "First line only");
        assert!(truncated); // More content exists
    }

    #[test]
    fn test_content_stats() {
        let content = "Hello world. This is a test! How are you?\n\nSecond paragraph here.";
        let stats = content_stats(content);

        // Words: Hello, world, This, is, a, test, How, are, you, Second, paragraph, here = 12
        assert_eq!(stats.words, 12);
        assert_eq!(stats.lines, 3);
        // Sentences: "world." + "test!" + "you?" + "here." = 4 sentence endings
        assert_eq!(stats.sentences, 4);
        assert_eq!(stats.paragraphs, 2);
    }

    #[test]
    fn test_content_stats_empty() {
        let stats = content_stats("");

        assert_eq!(stats.chars, 0);
        assert_eq!(stats.words, 0);
        assert_eq!(stats.lines, 1); // min 1
        assert_eq!(stats.sentences, 1); // min 1
        assert_eq!(stats.paragraphs, 1); // min 1
    }

    #[test]
    fn test_soft_trim_unicode() {
        let content = "你好世界！这是一个很长的中文字符串。".repeat(50);
        let config = SoftTrimConfig {
            max_chars: 100,
            ..Default::default()
        };
        let result = soft_trim(&content, &config);

        // Should not panic on unicode
        assert!(result.was_trimmed);
        // Content should be valid UTF-8
        assert!(result.content.is_ascii() || !result.content.is_empty());
    }

    #[test]
    fn test_compact_preview_empty() {
        let (preview, truncated) = compact_preview("", 100);
        assert!(preview.is_empty());
        assert!(!truncated);
    }

    #[test]
    fn test_compact_preview_whitespace_only() {
        let (preview, truncated) = compact_preview("   \n  \n  ", 100);
        assert!(preview.is_empty());
        assert!(!truncated);
    }
}
