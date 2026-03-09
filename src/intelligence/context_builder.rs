//! Memory-aware prompt construction engine (T8)
//!
//! Assembles retrieved memories into structured LLM prompts with configurable
//! token budgets, section priorities, and fill strategies.
//!
//! # Example
//!
//! ```rust,ignore
//! use engram::intelligence::context_builder::{
//!     ContextBuilder, PromptTemplate, Section, SimpleTokenCounter, Strategy,
//! };
//!
//! let counter = SimpleTokenCounter;
//! let builder = ContextBuilder::new(Box::new(counter));
//!
//! let template = PromptTemplate {
//!     sections: vec![
//!         Section { name: "System".into(), content: "You are helpful.".into(), max_tokens: 100, priority: 0 },
//!         Section { name: "Memories".into(), content: String::new(), max_tokens: 500, priority: 1 },
//!     ],
//!     total_budget: 600,
//!     separator: "\n\n---\n\n".into(),
//! };
//!
//! let result = builder.build(&template, &memories, Strategy::Greedy);
//! ```

use chrono::{DateTime, Utc};

// NOTE: For production use, tiktoken-rs can be integrated as the token counter
// implementation by wrapping its BPE encoder in a struct that implements TokenCounter.
// We intentionally keep this module dependency-free beyond std + chrono.

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A section in the prompt with its own token budget and priority.
///
/// - `priority = 0` is the **highest** priority (filled first in Greedy).
/// - `content` may be pre-filled (static system prompt) or empty (to be
///   filled by memories at build time).
#[derive(Debug, Clone)]
pub struct Section {
    /// Display name used as the section header in the rendered prompt.
    pub name: String,
    /// Fixed content for this section. Memories are appended after this
    /// when the builder fills the section.
    pub content: String,
    /// Maximum tokens allowed for this section (including fixed content).
    pub max_tokens: usize,
    /// Fill priority — lower number = higher priority.
    pub priority: u8,
}

/// Template defining prompt structure.
///
/// The builder uses `sections` to determine what goes into the prompt and
/// respects `total_budget` as a hard upper bound across all sections.
#[derive(Debug, Clone)]
pub struct PromptTemplate {
    /// Ordered list of sections to render.
    pub sections: Vec<Section>,
    /// Hard token ceiling across the entire rendered prompt.
    pub total_budget: usize,
    /// String inserted between non-empty sections. Defaults to `"\n\n---\n\n"`.
    pub separator: String,
}

impl Default for PromptTemplate {
    fn default() -> Self {
        Self {
            sections: Vec::new(),
            total_budget: 4096,
            separator: "\n\n---\n\n".to_string(),
        }
    }
}

/// Strategy for filling sections with memories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Sort sections by priority (ascending), fill each with memories until
    /// the section budget or total budget is exhausted.
    Greedy,
    /// Allocate `total_budget` proportionally across sections based on their
    /// `max_tokens` ratios, then fill each within its allocation.
    Balanced,
    /// Sort memories by `created_at` descending (newest first), then fill
    /// sections by priority (same as Greedy but with recency-sorted memories).
    Recency,
}

/// Abstraction for counting tokens in a string.
///
/// Implement this trait to plug in an accurate tokenizer such as tiktoken-rs.
pub trait TokenCounter: Send + Sync {
    fn count_tokens(&self, text: &str) -> usize;
}

/// Simple token estimator that assumes ~4 characters per token.
///
/// This is a rough heuristic suitable for budgeting purposes. For accurate
/// counting, implement [`TokenCounter`] using tiktoken-rs or a similar library.
pub struct SimpleTokenCounter;

impl TokenCounter for SimpleTokenCounter {
    fn count_tokens(&self, text: &str) -> usize {
        // ~4 chars per token is a well-known rule-of-thumb for English text.
        text.len() / 4
    }
}

/// Minimal memory representation used by the builder.
///
/// The builder only needs content and a timestamp; callers can project from
/// the full `crate::types::Memory` into this struct before calling `build`.
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub content: String,
    pub created_at: DateTime<Utc>,
}

impl MemoryEntry {
    pub fn new(content: impl Into<String>, created_at: DateTime<Utc>) -> Self {
        Self {
            content: content.into(),
            created_at,
        }
    }
}

/// Context builder that assembles memories into structured prompts.
pub struct ContextBuilder {
    counter: Box<dyn TokenCounter>,
}

impl ContextBuilder {
    /// Create a new builder with the given token counter.
    pub fn new(counter: Box<dyn TokenCounter>) -> Self {
        Self { counter }
    }

    /// Estimate token count for `text` using the internal counter.
    pub fn estimate_tokens(&self, text: &str) -> usize {
        self.counter.count_tokens(text)
    }

    /// Build a prompt string from `template`, filling memory slots with
    /// `memories` according to `strategy`.
    ///
    /// Returns the assembled prompt as a single `String`. Sections that end
    /// up empty (no fixed content and no memories fit) are omitted entirely.
    /// Sections whose content exceeds their `max_tokens` budget are truncated
    /// with `"...[truncated]"`.
    pub fn build(
        &self,
        template: &PromptTemplate,
        memories: &[MemoryEntry],
        strategy: Strategy,
    ) -> String {
        // Sort sections by priority (ascending = highest priority first).
        let mut sections: Vec<&Section> = template.sections.iter().collect();
        sections.sort_by_key(|s| s.priority);

        // Pre-sort memories according to strategy.
        let sorted_memories: Vec<&MemoryEntry> = match strategy {
            Strategy::Recency => {
                let mut m: Vec<&MemoryEntry> = memories.iter().collect();
                m.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                m
            }
            _ => memories.iter().collect(),
        };

        // Compute per-section token allocations.
        let allocations = self.compute_allocations(template, &sections, strategy);

        let mut rendered: Vec<String> = Vec::new();
        let mut total_used = 0usize;

        for (idx, section) in sections.iter().enumerate() {
            let section_budget =
                allocations[idx].min(template.total_budget.saturating_sub(total_used));

            // Build section text starting from fixed content.
            let mut section_text = section.content.clone();

            // Append memories that still fit.
            for memory in &sorted_memories {
                let candidate = if section_text.is_empty() {
                    memory.content.clone()
                } else {
                    format!("{}\n{}", section_text, memory.content)
                };

                let candidate_tokens = self.counter.count_tokens(&candidate);
                if candidate_tokens <= section_budget {
                    section_text = candidate;
                }
                // If over section budget, skip this memory and try the next.
            }

            // Skip empty sections entirely.
            if section_text.is_empty() {
                continue;
            }

            // Truncate if the section text (including fixed content) exceeds budget.
            let section_tokens = self.counter.count_tokens(&section_text);
            let final_text = if section_tokens > section_budget {
                self.truncate_to_budget(&section_text, section_budget)
            } else {
                section_text.clone()
            };

            // Check that adding this section stays within total budget.
            let separator_tokens = if rendered.is_empty() {
                0
            } else {
                self.counter.count_tokens(&template.separator)
            };
            let final_tokens = self.counter.count_tokens(&final_text);

            if total_used + separator_tokens + final_tokens > template.total_budget {
                // Try to fit a truncated version.
                let remaining = template
                    .total_budget
                    .saturating_sub(total_used + separator_tokens);
                if remaining == 0 {
                    break;
                }
                let truncated = self.truncate_to_budget(&final_text, remaining);
                if !truncated.is_empty() {
                    rendered.push(self.render_section(section, &truncated));
                    // total_used update omitted: we break immediately after.
                }
                break; // No room for further sections.
            }

            total_used += separator_tokens + final_tokens;
            rendered.push(self.render_section(section, &final_text));
        }

        rendered.join(&template.separator)
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Compute the effective token budget for each section, respecting `strategy`.
    fn compute_allocations(
        &self,
        template: &PromptTemplate,
        sections: &[&Section],
        strategy: Strategy,
    ) -> Vec<usize> {
        match strategy {
            Strategy::Balanced => {
                // Proportional: each section gets (its max_tokens / sum_of_max_tokens) * total_budget.
                let total_weight: usize = sections.iter().map(|s| s.max_tokens).sum();
                if total_weight == 0 {
                    return sections.iter().map(|_| 0).collect();
                }
                sections
                    .iter()
                    .map(|s| {
                        let ratio = s.max_tokens as f64 / total_weight as f64;
                        (ratio * template.total_budget as f64).floor() as usize
                    })
                    .collect()
            }
            // Greedy and Recency both use section.max_tokens directly.
            Strategy::Greedy | Strategy::Recency => sections.iter().map(|s| s.max_tokens).collect(),
        }
    }

    /// Truncate `text` so that it fits within `budget` tokens, appending
    /// `"...[truncated]"` to signal the cut.
    fn truncate_to_budget(&self, text: &str, budget: usize) -> String {
        const SUFFIX: &str = "...[truncated]";

        let suffix_tokens = self.counter.count_tokens(SUFFIX);

        // If the budget can't even fit the suffix, return just the suffix (or empty).
        if budget <= suffix_tokens {
            return if budget == 0 {
                String::new()
            } else {
                SUFFIX.to_string()
            };
        }

        let char_budget = (budget - suffix_tokens) * 4; // approximate chars for the main text

        if text.len() <= char_budget {
            // Already fits; only add suffix if strictly over token count.
            let tokens = self.counter.count_tokens(text);
            if tokens <= budget {
                return text.to_string();
            }
        }

        // Walk character boundaries to find a safe cut point.
        let mut end = char_budget.min(text.len());
        // Align to a char boundary.
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}{}", &text[..end], SUFFIX)
    }

    /// Render a single section as `"## {name}\n{content}"`.
    fn render_section(&self, section: &Section, content: &str) -> String {
        format!("## {}\n{}", section.name, content)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn make_counter() -> Box<dyn TokenCounter> {
        Box::new(SimpleTokenCounter)
    }

    fn make_memory(content: &str, days_ago: i64) -> MemoryEntry {
        let created_at =
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::days(-days_ago);
        MemoryEntry::new(content, created_at)
    }

    fn simple_template(total_budget: usize) -> PromptTemplate {
        PromptTemplate {
            sections: vec![
                Section {
                    name: "High Priority".into(),
                    content: String::new(),
                    max_tokens: total_budget / 2,
                    priority: 0,
                },
                Section {
                    name: "Low Priority".into(),
                    content: String::new(),
                    max_tokens: total_budget / 2,
                    priority: 1,
                },
            ],
            total_budget,
            separator: "\n\n---\n\n".into(),
        }
    }

    // Test 1: Greedy strategy fills high-priority section first.
    #[test]
    fn test_greedy_fills_high_priority_first() {
        let builder = ContextBuilder::new(make_counter());

        // Total budget = 40 tokens → each section gets 20.
        // Memory A alone costs 160 chars / 4 = 40 tokens — won't fit in any section.
        // Two small memories each 20 chars / 4 = 5 tokens — both fit in high-priority.
        let memories = vec![
            make_memory("AAAA AAAA AAAA AAAA", 2), // 20 chars = 5 tokens
            make_memory("BBBB BBBB BBBB BBBB", 3), // 20 chars = 5 tokens
        ];

        let template = simple_template(40);
        let result = builder.build(&template, &memories, Strategy::Greedy);

        // High-priority section should appear before low-priority.
        let high_pos = result.find("High Priority").unwrap_or(usize::MAX);
        let low_pos = result.find("Low Priority").unwrap_or(usize::MAX);
        assert!(
            high_pos < low_pos,
            "High-priority section must come before low-priority"
        );

        // Both memories fit — result should contain both.
        assert!(
            result.contains("AAAA"),
            "High-priority content must be present"
        );
    }

    // Test 2: Balanced strategy distributes proportionally.
    #[test]
    fn test_balanced_proportional_allocation() {
        let builder = ContextBuilder::new(make_counter());

        // Section A max_tokens = 100, Section B max_tokens = 300.
        // Total weight = 400, total_budget = 200.
        // Section A gets (100/400)*200 = 50 tokens, Section B gets (300/400)*200 = 150 tokens.
        let template = PromptTemplate {
            sections: vec![
                Section {
                    name: "Small".into(),
                    content: String::new(),
                    max_tokens: 100,
                    priority: 0,
                },
                Section {
                    name: "Large".into(),
                    content: String::new(),
                    max_tokens: 300,
                    priority: 1,
                },
            ],
            total_budget: 200,
            separator: "\n\n---\n\n".into(),
        };

        // A memory of exactly 140 chars = 35 tokens — fits in Large (150) but not Small (50).
        let memories = vec![make_memory(&"X".repeat(140), 1)];
        let result = builder.build(&template, &memories, Strategy::Balanced);

        // The memory should appear in Large section (high allocation).
        assert!(result.contains("Large"), "Large section must be rendered");
        assert!(
            result.contains(&"X".repeat(140)),
            "Memory must fit in the Large section"
        );
    }

    // Test 3: Recency strategy prefers newer memories.
    #[test]
    fn test_recency_prefers_newer_memories() {
        let builder = ContextBuilder::new(make_counter());

        // Budget tight enough for only one memory per section.
        // old_memory is 60 chars = 15 tokens; new_memory same size.
        // Section max_tokens = 16 — only one fits.
        let template = PromptTemplate {
            sections: vec![Section {
                name: "Context".into(),
                content: String::new(),
                max_tokens: 20, // fits ~1 memory (15 tokens) + header
                priority: 0,
            }],
            total_budget: 40,
            separator: "\n\n---\n\n".into(),
        };

        // older memory: days_ago = 10; newer memory: days_ago = 0.
        let memories = vec![
            make_memory("OLD_MEMORY_CONTENT_HERE_PADDED", 10), // older
            make_memory("NEW_MEMORY_CONTENT_HERE_PADDED", 0),  // newer
        ];

        let result = builder.build(&template, &memories, Strategy::Recency);

        // The newer memory must appear (it is tried first due to recency sort).
        assert!(
            result.contains("NEW_MEMORY"),
            "Recency strategy must prefer newest memory"
        );
    }

    // Test 4: Overflow truncation appends "...[truncated]".
    #[test]
    fn test_overflow_truncation() {
        let builder = ContextBuilder::new(make_counter());

        // Section with very small budget: 5 tokens.
        // Fixed content is 80 chars = 20 tokens — will be truncated.
        let template = PromptTemplate {
            sections: vec![Section {
                name: "Tiny".into(),
                content: "A".repeat(80), // 20 tokens, exceeds budget of 5
                max_tokens: 5,
                priority: 0,
            }],
            total_budget: 50,
            separator: "\n\n---\n\n".into(),
        };

        let result = builder.build(&template, &[], Strategy::Greedy);
        assert!(
            result.contains("...[truncated]"),
            "Overflowed section must end with ...[truncated]; got: {result}"
        );
    }

    // Test 5: Empty sections (no fixed content and no memories fit) are skipped.
    #[test]
    fn test_empty_sections_skipped() {
        let builder = ContextBuilder::new(make_counter());

        let template = PromptTemplate {
            sections: vec![
                Section {
                    name: "Present".into(),
                    content: "I have content.".into(),
                    max_tokens: 100,
                    priority: 0,
                },
                Section {
                    name: "Empty".into(),
                    content: String::new(), // no fixed content
                    max_tokens: 100,
                    priority: 1,
                },
            ],
            total_budget: 500,
            separator: "\n\n---\n\n".into(),
        };

        // Pass no memories — the "Empty" section will have nothing to render.
        let result = builder.build(&template, &[], Strategy::Greedy);

        assert!(
            result.contains("Present"),
            "Non-empty section must be rendered"
        );
        assert!(
            !result.contains("Empty"),
            "Truly empty section must be skipped"
        );
    }

    // Test 6: SimpleTokenCounter accuracy.
    #[test]
    fn test_simple_token_counter_accuracy() {
        let counter = SimpleTokenCounter;

        // 0 chars → 0 tokens.
        assert_eq!(counter.count_tokens(""), 0);

        // 4 chars → 1 token.
        assert_eq!(counter.count_tokens("abcd"), 1);

        // 8 chars → 2 tokens.
        assert_eq!(counter.count_tokens("abcdefgh"), 2);

        // 100 chars → 25 tokens.
        assert_eq!(counter.count_tokens(&"a".repeat(100)), 25);

        // Non-divisible: 10 chars → 2 (integer division).
        assert_eq!(counter.count_tokens("abcdefghij"), 2);
    }

    // Test 7: Total budget is respected across all sections.
    #[test]
    fn test_total_budget_respected() {
        let builder = ContextBuilder::new(make_counter());

        // Each section allows 1000 tokens, but total_budget = 100.
        // The rendered output (headers + separator + content) must fit within 100 tokens.
        let budget = 100;
        let template = PromptTemplate {
            sections: vec![
                Section {
                    name: "A".into(),
                    content: String::new(),
                    max_tokens: 1000,
                    priority: 0,
                },
                Section {
                    name: "B".into(),
                    content: String::new(),
                    max_tokens: 1000,
                    priority: 1,
                },
            ],
            total_budget: budget,
            separator: "\n\n---\n\n".into(),
        };

        // Each memory is 40 chars = 10 tokens — many of them to stress the budget.
        let memories: Vec<MemoryEntry> = (0..20)
            .map(|i| make_memory(&format!("{:0>40}", i), i as i64))
            .collect();

        let result = builder.build(&template, &memories, Strategy::Greedy);

        // The output token count must not exceed total_budget.
        let token_count = SimpleTokenCounter.count_tokens(&result);
        assert!(
            token_count <= budget,
            "Output tokens ({token_count}) must not exceed total_budget ({budget})"
        );
    }

    // Bonus: estimate_tokens delegates to the counter correctly.
    #[test]
    fn test_estimate_tokens_delegation() {
        let builder = ContextBuilder::new(make_counter());
        assert_eq!(
            builder.estimate_tokens("hello"),
            SimpleTokenCounter.count_tokens("hello")
        );
        assert_eq!(builder.estimate_tokens(""), 0);
    }
}
