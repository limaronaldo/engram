//! BM25 full-text search implementation (RML-876)
//!
//! Uses SQLite FTS5 with BM25 ranking for high-quality keyword search.

use chrono::Utc;
use rusqlite::Connection;

use crate::error::Result;
use crate::storage::filter::{parse_filter, SqlBuilder};
use crate::storage::queries::{load_tags, memory_from_row};
use crate::types::{MatchInfo, Memory, MemoryScope, SearchStrategy};

/// BM25 search result with score
#[derive(Debug)]
pub struct Bm25Result {
    pub memory: Memory,
    pub score: f32,
    pub matched_terms: Vec<String>,
    pub highlights: Vec<String>,
}

/// Perform BM25 search using FTS5
pub fn bm25_search(
    conn: &Connection,
    query: &str,
    limit: i64,
    explain: bool,
) -> Result<Vec<Bm25Result>> {
    bm25_search_with_options(conn, query, limit, explain, None)
}

/// Perform BM25 search with optional scope filtering
pub fn bm25_search_with_options(
    conn: &Connection,
    query: &str,
    limit: i64,
    explain: bool,
    scope: Option<&MemoryScope>,
) -> Result<Vec<Bm25Result>> {
    bm25_search_with_filter(conn, query, limit, explain, scope, None)
}

/// Perform BM25 search with optional scope and advanced filter
pub fn bm25_search_with_filter(
    conn: &Connection,
    query: &str,
    limit: i64,
    explain: bool,
    scope: Option<&MemoryScope>,
    filter: Option<&serde_json::Value>,
) -> Result<Vec<Bm25Result>> {
    bm25_search_full(conn, query, limit, explain, scope, filter, false)
}

/// Perform BM25 search with all options including transcript exclusion
pub fn bm25_search_full(
    conn: &Connection,
    query: &str,
    limit: i64,
    explain: bool,
    scope: Option<&MemoryScope>,
    filter: Option<&serde_json::Value>,
    include_transcripts: bool,
) -> Result<Vec<Bm25Result>> {
    // Escape special FTS5 characters
    let escaped_query = escape_fts5_query(query);
    let now = Utc::now().to_rfc3339();

    // Note: snippet() is not available with external content FTS5 tables
    // We generate highlights manually from the content instead
    let mut sql = String::from(
        r#"
        SELECT
            m.id, m.content, m.memory_type, m.importance, m.access_count,
            m.created_at, m.updated_at, m.last_accessed_at, m.owner_id,
            m.visibility, m.version, m.has_embedding, m.metadata,
            m.scope_type, m.scope_id, m.expires_at,
            bm25(memories_fts) as score
        FROM memories_fts fts
        JOIN memories m ON fts.rowid = m.id
        WHERE memories_fts MATCH ? AND m.valid_to IS NULL
          AND (m.expires_at IS NULL OR m.expires_at > ?)
    "#,
    );

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(escaped_query), Box::new(now)];

    // Exclude transcript chunks by default (unless include_transcripts is true)
    if !include_transcripts {
        sql.push_str(" AND m.memory_type != 'transcript_chunk'");
    }

    // Add advanced filter (RML-932)
    if let Some(filter_json) = filter {
        let filter_expr = parse_filter(filter_json)?;
        let mut builder = SqlBuilder::new();
        let filter_sql = builder.build_filter(&filter_expr)?;
        sql.push_str(" AND ");
        sql.push_str(&filter_sql);
        for param in builder.take_params() {
            params.push(param);
        }
    }

    // Add scope filter
    if let Some(scope) = scope {
        sql.push_str(" AND m.scope_type = ?");
        params.push(Box::new(scope.scope_type().to_string()));
        if let Some(scope_id) = scope.scope_id() {
            sql.push_str(" AND m.scope_id = ?");
            params.push(Box::new(scope_id.to_string()));
        } else {
            sql.push_str(" AND m.scope_id IS NULL");
        }
    }

    sql.push_str(" ORDER BY bm25(memories_fts) LIMIT ?");
    params.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let mut results = Vec::new();

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();

    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let memory = memory_from_row(row)?;
        let score: f32 = row.get("score")?;
        Ok((memory, score))
    })?;

    for row in rows {
        let (mut memory, score) = row?;
        memory.tags = load_tags(conn, memory.id)?;

        // BM25 returns negative scores (closer to 0 = better)
        // Normalize to positive 0-1 range
        let normalized_score = 1.0 / (1.0 + score.abs());

        let matched_terms = if explain {
            extract_matched_terms(query, &memory.content)
        } else {
            vec![]
        };

        let highlights = if explain {
            generate_highlights(query, &memory.content)
        } else {
            vec![]
        };

        results.push(Bm25Result {
            memory,
            score: normalized_score,
            matched_terms,
            highlights,
        });
    }

    Ok(results)
}

/// Phrase search using FTS5 phrase queries
pub fn phrase_search(conn: &Connection, phrase: &str, limit: i64) -> Result<Vec<Bm25Result>> {
    // Wrap in quotes for exact phrase matching
    let query = format!("\"{}\"", phrase.replace('"', ""));
    bm25_search(conn, &query, limit, false)
}

/// Proximity search using NEAR operator
pub fn proximity_search(
    conn: &Connection,
    terms: &[&str],
    max_distance: i32,
    limit: i64,
) -> Result<Vec<Bm25Result>> {
    if terms.is_empty() {
        return Ok(vec![]);
    }

    let escaped_terms: Vec<String> = terms.iter().map(|t| escape_fts5_term(t)).collect();
    let query = format!("NEAR({}, {})", escaped_terms.join(" "), max_distance);
    bm25_search(conn, &query, limit, false)
}

/// Field-specific search (content:, tags:)
pub fn field_search(
    conn: &Connection,
    field: &str,
    query: &str,
    limit: i64,
) -> Result<Vec<Bm25Result>> {
    let valid_fields = ["content", "tags", "metadata"];
    if !valid_fields.contains(&field) {
        return bm25_search(conn, query, limit, false);
    }

    let field_query = format!("{}: {}", field, escape_fts5_query(query));
    bm25_search(conn, &field_query, limit, false)
}

/// Escape special FTS5 characters in query
///
/// FTS5 has several special characters and operators:
/// - Quotes: `"phrase"` for exact phrase matching
/// - Operators: `AND`, `OR`, `NOT` (case-sensitive in FTS5)
/// - Prefix: `term*` for prefix matching
/// - Column filter: `column:term`
/// - NEAR: `NEAR(term1 term2, distance)`
/// - Special chars: `(){}[]^~+-`
///
/// This function safely escapes user input to prevent FTS5 injection.
fn escape_fts5_query(query: &str) -> String {
    // Handle empty or whitespace-only input
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Handle quoted phrases - but validate they're properly closed
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() > 2 {
        // Escape any internal quotes
        let inner = &trimmed[1..trimmed.len() - 1];
        let escaped_inner = inner.replace('"', "\"\"");
        return format!("\"{}\"", escaped_inner);
    }

    // Split into terms and escape each
    trimmed
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(escape_fts5_term)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escape a single FTS5 term
///
/// Handles all FTS5 special characters:
/// - `"` - quote (escaped by doubling)
/// - `*` - prefix wildcard
/// - `()` - grouping
/// - `{}[]` - column filter syntax
/// - `^` - boost operator
/// - `~` - NOT operator (in some contexts)
/// - `:` - column prefix
/// - `+` - required term
/// - `-` - excluded term
fn escape_fts5_term(term: &str) -> String {
    // Empty term check
    if term.is_empty() {
        return String::new();
    }

    // FTS5 special characters that need quoting
    // Note: We include all chars that have special meaning in FTS5
    let special = [
        '"', '*', '(', ')', '{', '}', '[', ']', '^', '~', ':', '+', '-',
    ];

    // Check if term needs quoting
    let needs_quotes = term
        .chars()
        .any(|c| special.contains(&c) || c.is_whitespace());

    // Also check for FTS5 boolean operators (case-sensitive)
    let is_operator = matches!(term, "AND" | "OR" | "NOT" | "NEAR");

    if needs_quotes || is_operator {
        let mut escaped = String::with_capacity(term.len() + 4);
        escaped.push('"');
        for c in term.chars() {
            if c == '"' {
                escaped.push_str("\"\""); // Double quotes to escape
            } else {
                escaped.push(c);
            }
        }
        escaped.push('"');
        escaped
    } else {
        term.to_string()
    }
}

/// Extract which query terms matched in the content
fn extract_matched_terms(query: &str, content: &str) -> Vec<String> {
    let content_lower = content.to_lowercase();
    query
        .split_whitespace()
        .filter(|term| {
            let term_lower = term.to_lowercase();
            // Remove FTS5 operators for matching
            let clean_term =
                term_lower.trim_matches(|c| c == '"' || c == '*' || c == '+' || c == '-');
            content_lower.contains(clean_term)
        })
        .map(String::from)
        .collect()
}

/// Generate highlight snippets from content (since FTS5 snippet() doesn't work with external content)
fn generate_highlights(query: &str, content: &str) -> Vec<String> {
    let content_lower = content.to_lowercase();
    let terms: Vec<&str> = query
        .split_whitespace()
        .map(|t| t.trim_matches(|c| c == '"' || c == '*' || c == '+' || c == '-'))
        .filter(|t| !t.is_empty())
        .collect();

    if terms.is_empty() {
        return vec![];
    }

    // Find the first matching term and extract context around it
    for term in &terms {
        let term_lower = term.to_lowercase();
        if let Some(pos) = content_lower.find(&term_lower) {
            let start = pos.saturating_sub(30);
            let end = (pos + term.len() + 30).min(content.len());

            // Find word boundaries
            let snippet_start = content[..start].rfind(' ').map(|p| p + 1).unwrap_or(start);
            let snippet_end = content[end..].find(' ').map(|p| end + p).unwrap_or(end);

            let mut snippet = String::new();
            if snippet_start > 0 {
                snippet.push_str("...");
            }
            snippet.push_str(content[snippet_start..snippet_end].trim());
            if snippet_end < content.len() {
                snippet.push_str("...");
            }
            return vec![snippet];
        }
    }

    vec![]
}

/// Convert BM25 results to MatchInfo
impl Bm25Result {
    pub fn to_match_info(&self) -> MatchInfo {
        MatchInfo {
            strategy: SearchStrategy::KeywordOnly,
            matched_terms: self.matched_terms.clone(),
            highlights: self.highlights.clone(),
            semantic_score: None,
            keyword_score: Some(self.score),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // FTS5 Term Escaping Tests
    // =========================================================================

    #[test]
    fn test_escape_fts5_term_simple() {
        assert_eq!(escape_fts5_term("hello"), "hello");
        assert_eq!(escape_fts5_term("world"), "world");
        assert_eq!(escape_fts5_term("rust123"), "rust123");
    }

    #[test]
    fn test_escape_fts5_term_with_spaces() {
        assert_eq!(escape_fts5_term("hello world"), "\"hello world\"");
        assert_eq!(escape_fts5_term("  spaces  "), "\"  spaces  \"");
    }

    #[test]
    fn test_escape_fts5_term_with_quotes() {
        assert_eq!(escape_fts5_term("test\"quote"), "\"test\"\"quote\"");
        assert_eq!(escape_fts5_term("\"quoted\""), "\"\"\"quoted\"\"\"");
    }

    #[test]
    fn test_escape_fts5_term_special_chars() {
        // Prefix wildcard
        assert_eq!(escape_fts5_term("test*"), "\"test*\"");
        // Grouping
        assert_eq!(escape_fts5_term("(group)"), "\"(group)\"");
        // Column filter
        assert_eq!(escape_fts5_term("content:term"), "\"content:term\"");
        // Boost
        assert_eq!(escape_fts5_term("term^2"), "\"term^2\"");
        // Plus/minus
        assert_eq!(escape_fts5_term("+required"), "\"+required\"");
        assert_eq!(escape_fts5_term("-excluded"), "\"-excluded\"");
    }

    #[test]
    fn test_escape_fts5_term_operators() {
        // FTS5 boolean operators should be quoted to be treated as literals
        assert_eq!(escape_fts5_term("AND"), "\"AND\"");
        assert_eq!(escape_fts5_term("OR"), "\"OR\"");
        assert_eq!(escape_fts5_term("NOT"), "\"NOT\"");
        assert_eq!(escape_fts5_term("NEAR"), "\"NEAR\"");
        // Lowercase versions are not operators
        assert_eq!(escape_fts5_term("and"), "and");
        assert_eq!(escape_fts5_term("or"), "or");
    }

    #[test]
    fn test_escape_fts5_term_empty() {
        assert_eq!(escape_fts5_term(""), "");
    }

    // =========================================================================
    // FTS5 Query Escaping Tests
    // =========================================================================

    #[test]
    fn test_escape_fts5_query_simple() {
        assert_eq!(escape_fts5_query("hello world"), "hello world");
        assert_eq!(escape_fts5_query("single"), "single");
    }

    #[test]
    fn test_escape_fts5_query_quoted_phrase() {
        assert_eq!(escape_fts5_query("\"exact phrase\""), "\"exact phrase\"");
        // Internal quotes are escaped
        assert_eq!(
            escape_fts5_query("\"phrase with \"quotes\"\""),
            "\"phrase with \"\"quotes\"\"\""
        );
    }

    #[test]
    fn test_escape_fts5_query_whitespace() {
        assert_eq!(escape_fts5_query(""), "");
        assert_eq!(escape_fts5_query("   "), "");
        assert_eq!(escape_fts5_query("  hello  world  "), "hello world");
    }

    #[test]
    fn test_escape_fts5_query_injection_attempts() {
        // Attempt to inject FTS5 operators - OR gets quoted, parens in terms get quoted
        assert_eq!(
            escape_fts5_query("hello OR (drop table)"),
            "hello \"OR\" \"(drop\" \"table)\""
        );
        // Attempt to inject column filter - colon causes quoting
        assert_eq!(
            escape_fts5_query("content:malicious"),
            "\"content:malicious\""
        );
        // Attempt to use NEAR operator - NEAR( is one token, gets quoted
        assert_eq!(
            escape_fts5_query("NEAR(term1 term2, 5)"),
            "\"NEAR(term1\" term2, \"5)\""
        );
    }

    #[test]
    fn test_escape_fts5_query_real_world() {
        // Common search patterns users might enter
        assert_eq!(escape_fts5_query("user@example.com"), "user@example.com");
        assert_eq!(escape_fts5_query("file.rs"), "file.rs");
        assert_eq!(escape_fts5_query("C++"), "\"C++\"");
        assert_eq!(escape_fts5_query("node.js"), "node.js");
        assert_eq!(escape_fts5_query("@username"), "@username");
        // URL-like patterns - colon causes entire term to be quoted
        assert_eq!(
            escape_fts5_query("https://example.com"),
            "\"https://example.com\""
        );
    }

    // =========================================================================
    // Match Extraction Tests
    // =========================================================================

    #[test]
    fn test_extract_matched_terms() {
        let terms = extract_matched_terms("hello world", "Hello there, World!");
        assert!(terms.contains(&"hello".to_string()));
        assert!(terms.contains(&"world".to_string()));
    }

    #[test]
    fn test_extract_matched_terms_partial() {
        let terms = extract_matched_terms("rust programming", "Rust is a programming language");
        assert!(terms.contains(&"rust".to_string()));
        assert!(terms.contains(&"programming".to_string()));
    }

    #[test]
    fn test_extract_matched_terms_no_match() {
        let terms = extract_matched_terms("xyz abc", "Hello world");
        assert!(terms.is_empty());
    }

    // =========================================================================
    // Highlight Generation Tests
    // =========================================================================

    #[test]
    fn test_generate_highlights() {
        let highlights = generate_highlights("test", "This is a test string for testing");
        assert!(!highlights.is_empty());
        assert!(highlights[0].contains("test"));
    }

    #[test]
    fn test_generate_highlights_no_match() {
        let highlights = generate_highlights("xyz", "Hello world");
        assert!(highlights.is_empty());
    }
}
