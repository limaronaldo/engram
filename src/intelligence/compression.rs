//! Context Compression Engine (Phase 2 - ENG-34)
//!
//! Provides token counting and context budget management for LLM interactions.
//! Uses tiktoken-rs for accurate token counting with explicit error handling.

use crate::error::{EngramError, Result};
use serde::{Deserialize, Serialize};

/// Compression strategy for memory content
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompressionStrategy {
    /// Raw content, no compression (default)
    #[default]
    None,
    /// Keep first 60% + last 30% with ellipsis (uses soft_trim)
    HeadTail,
    /// LLM-generated summary (creates new Summary memory)
    Summary,
}

/// Supported encoding types for token counting
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenEncoding {
    /// cl100k_base - GPT-4, GPT-4-turbo, text-embedding-3-*
    Cl100kBase,
    /// o200k_base - GPT-4o, GPT-4o-mini
    O200kBase,
}

impl TokenEncoding {
    pub fn as_str(&self) -> &'static str {
        match self {
            TokenEncoding::Cl100kBase => "cl100k_base",
            TokenEncoding::O200kBase => "o200k_base",
        }
    }
}

/// Detect the appropriate encoding for a model name
pub fn detect_encoding(model: &str) -> Option<TokenEncoding> {
    let model_lower = model.to_lowercase();

    // GPT-4o and GPT-4o-mini use o200k_base
    if model_lower.contains("gpt-4o") {
        return Some(TokenEncoding::O200kBase);
    }

    // GPT-4, GPT-4-turbo use cl100k_base
    if model_lower.contains("gpt-4") || model_lower.contains("gpt-3.5") {
        return Some(TokenEncoding::Cl100kBase);
    }

    // text-embedding models use cl100k_base
    if model_lower.contains("text-embedding") {
        return Some(TokenEncoding::Cl100kBase);
    }

    // Claude models - use cl100k_base as approximation
    // (Claude's actual tokenizer is different but cl100k is close enough for budgeting)
    if model_lower.contains("claude") {
        return Some(TokenEncoding::Cl100kBase);
    }

    // OpenRouter prefixed models
    if let Some(stripped) = model_lower.strip_prefix("openai/") {
        return detect_encoding(stripped);
    }
    if model_lower.starts_with("anthropic/") {
        return Some(TokenEncoding::Cl100kBase);
    }

    None
}

/// Parse encoding string to TokenEncoding
pub fn parse_encoding(encoding: &str) -> Option<TokenEncoding> {
    match encoding.to_lowercase().as_str() {
        "cl100k_base" | "cl100k" => Some(TokenEncoding::Cl100kBase),
        "o200k_base" | "o200k" => Some(TokenEncoding::O200kBase),
        _ => None,
    }
}

/// Count tokens in text using the specified model or encoding.
///
/// # Arguments
/// * `text` - The text to count tokens for
/// * `model` - Model name (e.g., "gpt-4", "gpt-4o", "claude-3-opus")
/// * `encoding` - Optional encoding override (e.g., "cl100k_base", "o200k_base")
///
/// # Returns
/// * `Ok(usize)` - Number of tokens
/// * `Err` - If model is unknown AND no encoding provided
///
/// # Errors
/// This function will NOT silently fall back to chars/4. If the model is unknown
/// and no encoding is provided, it returns an error with a helpful message.
pub fn count_tokens(text: &str, model: &str, encoding: Option<&str>) -> Result<usize> {
    // First try explicit encoding override
    let token_encoding = if let Some(enc) = encoding {
        parse_encoding(enc).ok_or_else(|| {
            EngramError::InvalidInput(format!(
                "Unknown encoding '{}'. Supported: cl100k_base, o200k_base",
                enc
            ))
        })?
    } else {
        // Try to detect from model name
        detect_encoding(model).ok_or_else(|| {
            EngramError::InvalidInput(format!(
                "Unknown model '{}'. Provide 'encoding' parameter (cl100k_base or o200k_base) or use a known model (gpt-4, gpt-4o, claude-*, text-embedding-*).",
                model
            ))
        })?
    };

    // Use tiktoken-rs to count tokens
    let bpe = match token_encoding {
        TokenEncoding::Cl100kBase => tiktoken_rs::cl100k_base(),
        TokenEncoding::O200kBase => tiktoken_rs::o200k_base(),
    };

    match bpe {
        Ok(encoder) => Ok(encoder.encode_with_special_tokens(text).len()),
        Err(e) => Err(EngramError::Internal(format!(
            "Failed to initialize tokenizer: {}",
            e
        ))),
    }
}

/// Input for context budget checking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudgetInput {
    /// Memory IDs to check
    pub memory_ids: Vec<i64>,
    /// Model name (required)
    pub model: String,
    /// Optional encoding override
    pub encoding: Option<String>,
    /// Token budget to check against
    pub budget: usize,
}

/// Result of context budget check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudgetResult {
    /// Total tokens across all memories
    pub total_tokens: usize,
    /// The budget that was checked against
    pub budget: usize,
    /// Remaining tokens (budget - total, or 0 if over)
    pub remaining: usize,
    /// Whether total exceeds budget
    pub over_budget: bool,
    /// Number of memories counted
    pub memories_counted: usize,
    /// Model used for counting
    pub model_used: String,
    /// Encoding used for counting
    pub encoding_used: String,
    /// Suggestions if over budget
    pub suggestions: Vec<String>,
    /// Per-memory token counts
    pub memory_tokens: Vec<MemoryTokenCount>,
}

/// Token count for a single memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryTokenCount {
    pub memory_id: i64,
    pub tokens: usize,
    pub content_preview: String,
}

impl ContextBudgetResult {
    pub fn new(
        total_tokens: usize,
        budget: usize,
        model: &str,
        encoding: TokenEncoding,
        memory_tokens: Vec<MemoryTokenCount>,
    ) -> Self {
        let over_budget = total_tokens > budget;
        let remaining = if over_budget {
            0
        } else {
            budget - total_tokens
        };

        let mut suggestions = Vec::new();
        if over_budget {
            let excess = total_tokens - budget;
            suggestions.push(format!(
                "Over budget by {} tokens ({:.1}% of budget)",
                excess,
                (excess as f64 / budget as f64) * 100.0
            ));

            // Find largest memories
            let mut sorted = memory_tokens.clone();
            sorted.sort_by(|a, b| b.tokens.cmp(&a.tokens));

            if let Some(largest) = sorted.first() {
                suggestions.push(format!(
                    "Largest memory: id={} ({} tokens) - consider summarizing",
                    largest.memory_id, largest.tokens
                ));
            }

            suggestions.push("Use memory_summarize to compress large memories".to_string());
            suggestions.push("Use memory_archive_old to batch summarize old memories".to_string());
        }

        Self {
            total_tokens,
            budget,
            remaining,
            over_budget,
            memories_counted: memory_tokens.len(),
            model_used: model.to_string(),
            encoding_used: encoding.as_str().to_string(),
            suggestions,
            memory_tokens,
        }
    }
}

/// Check token budget for a set of memories
pub fn check_context_budget(
    contents: &[(i64, String)],
    model: &str,
    encoding: Option<&str>,
    budget: usize,
) -> Result<ContextBudgetResult> {
    // Determine encoding (validates model/encoding)
    let token_encoding = if let Some(enc) = encoding {
        parse_encoding(enc).ok_or_else(|| {
            EngramError::InvalidInput(format!(
                "Unknown encoding '{}'. Supported: cl100k_base, o200k_base",
                enc
            ))
        })?
    } else {
        detect_encoding(model).ok_or_else(|| {
            EngramError::InvalidInput(format!(
                "Unknown model '{}'. Provide 'encoding' parameter (cl100k_base or o200k_base) or use a known model.",
                model
            ))
        })?
    };

    let bpe = match token_encoding {
        TokenEncoding::Cl100kBase => tiktoken_rs::cl100k_base(),
        TokenEncoding::O200kBase => tiktoken_rs::o200k_base(),
    }
    .map_err(|e| EngramError::Internal(format!("Failed to initialize tokenizer: {}", e)))?;

    let mut memory_tokens = Vec::new();
    let mut total_tokens = 0;

    for (id, content) in contents {
        let tokens = bpe.encode_with_special_tokens(content).len();
        total_tokens += tokens;

        // Create preview (first 50 chars)
        let preview = if content.len() > 50 {
            format!("{}...", &content[..50])
        } else {
            content.clone()
        };

        memory_tokens.push(MemoryTokenCount {
            memory_id: *id,
            tokens,
            content_preview: preview,
        });
    }

    Ok(ContextBudgetResult::new(
        total_tokens,
        budget,
        model,
        token_encoding,
        memory_tokens,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_encoding() {
        assert_eq!(detect_encoding("gpt-4"), Some(TokenEncoding::Cl100kBase));
        assert_eq!(
            detect_encoding("gpt-4-turbo"),
            Some(TokenEncoding::Cl100kBase)
        );
        assert_eq!(detect_encoding("gpt-4o"), Some(TokenEncoding::O200kBase));
        assert_eq!(
            detect_encoding("gpt-4o-mini"),
            Some(TokenEncoding::O200kBase)
        );
        assert_eq!(
            detect_encoding("claude-3-opus"),
            Some(TokenEncoding::Cl100kBase)
        );
        assert_eq!(
            detect_encoding("text-embedding-3-small"),
            Some(TokenEncoding::Cl100kBase)
        );
        assert_eq!(detect_encoding("unknown-model"), None);
    }

    #[test]
    fn test_count_tokens_known_model() {
        let result = count_tokens("Hello, world!", "gpt-4", None);
        assert!(result.is_ok());
        assert!(result.unwrap() > 0);
    }

    #[test]
    fn test_count_tokens_unknown_model_no_encoding() {
        let result = count_tokens("Hello, world!", "unknown-model", None);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unknown model"));
    }

    #[test]
    fn test_count_tokens_unknown_model_with_encoding() {
        let result = count_tokens("Hello, world!", "unknown-model", Some("cl100k_base"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_context_budget_under() {
        let contents = vec![
            (1, "Hello world".to_string()),
            (2, "Test content".to_string()),
        ];
        let result = check_context_budget(&contents, "gpt-4", None, 1000).unwrap();
        assert!(!result.over_budget);
        assert!(result.remaining > 0);
        assert_eq!(result.memories_counted, 2);
    }

    #[test]
    fn test_context_budget_over() {
        let contents = vec![(1, "A".repeat(10000))];
        let result = check_context_budget(&contents, "gpt-4", None, 100).unwrap();
        assert!(result.over_budget);
        assert_eq!(result.remaining, 0);
        assert!(!result.suggestions.is_empty());
    }
}
