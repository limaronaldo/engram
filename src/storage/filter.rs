//! Advanced metadata filter syntax for memory queries (RML-932)
//!
//! Supports complex queries with AND, OR, comparison operators:
//!
//! ```json
//! {
//!   "AND": [
//!     {"metadata.project": {"eq": "engram"}},
//!     {"metadata.priority": {"gte": 3}},
//!     {"OR": [
//!       {"tags": {"contains": "rust"}},
//!       {"tags": {"contains": "performance"}}
//!     ]}
//!   ]
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{EngramError, Result};

/// A filter expression that can be evaluated against a memory
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum FilterExpr {
    /// Logical AND of multiple expressions
    And {
        #[serde(rename = "AND")]
        conditions: Vec<FilterExpr>,
    },
    /// Logical OR of multiple expressions
    Or {
        #[serde(rename = "OR")]
        conditions: Vec<FilterExpr>,
    },
    /// A single field condition
    Condition(FieldCondition),
}

/// A condition on a specific field
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(transparent)]
pub struct FieldCondition {
    #[serde(flatten)]
    pub inner: std::collections::HashMap<String, FilterOp>,
}

/// Filter operation with operator and value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum FilterOp {
    /// Equal to value
    Eq(Value),
    /// Not equal to value
    Neq(Value),
    /// Greater than value
    Gt(Value),
    /// Greater than or equal to value
    Gte(Value),
    /// Less than value
    Lt(Value),
    /// Less than or equal to value
    Lte(Value),
    /// Array/string contains value
    Contains(Value),
    /// Array/string does not contain value
    #[serde(rename = "not_contains")]
    NotContains(Value),
    /// Field exists (is not null)
    Exists(bool),
    /// Legacy: direct value means equality (backwards compatible)
    #[serde(untagged)]
    Direct(Value),
}

/// Supported field paths for filtering
#[derive(Debug, Clone, PartialEq)]
pub enum FieldPath {
    /// Memory content
    Content,
    /// Memory type (note, todo, issue, etc.)
    MemoryType,
    /// Importance score (0-1)
    Importance,
    /// Tags array
    Tags,
    /// Created timestamp
    CreatedAt,
    /// Updated timestamp
    UpdatedAt,
    /// Scope type
    ScopeType,
    /// Scope ID
    ScopeId,
    /// Workspace
    Workspace,
    /// Tier (permanent/daily)
    Tier,
    /// Nested metadata field (e.g., "metadata.project" or "metadata.config.timeout")
    Metadata(String),
}

/// Validate that a metadata JSON path contains only safe characters.
/// Allows alphanumeric, underscore, dot (for nested paths), and hyphen.
/// Rejects any characters that could be used for SQL injection (quotes, brackets, etc).
fn validate_json_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Err(EngramError::InvalidInput(
            "Metadata path cannot be empty".to_string(),
        ));
    }

    // Only allow safe characters: alphanumeric, underscore, dot, hyphen
    // This prevents SQL injection via malicious paths like: foo'); DROP TABLE memories; --
    for ch in path.chars() {
        if !ch.is_alphanumeric() && ch != '_' && ch != '.' && ch != '-' {
            return Err(EngramError::InvalidInput(format!(
                "Invalid character '{}' in metadata path '{}'. Only alphanumeric, underscore, dot, and hyphen are allowed.",
                ch, path
            )));
        }
    }

    // Prevent path traversal patterns
    if path.contains("..") {
        return Err(EngramError::InvalidInput(
            "Metadata path cannot contain '..'".to_string(),
        ));
    }

    // Path shouldn't start or end with a dot
    if path.starts_with('.') || path.ends_with('.') {
        return Err(EngramError::InvalidInput(
            "Metadata path cannot start or end with '.'".to_string(),
        ));
    }

    Ok(())
}

impl FieldPath {
    /// Parse a field path string into a FieldPath enum
    pub fn parse(path: &str) -> Result<Self> {
        match path {
            "content" => Ok(FieldPath::Content),
            "memory_type" | "type" => Ok(FieldPath::MemoryType),
            "importance" => Ok(FieldPath::Importance),
            "tags" => Ok(FieldPath::Tags),
            "created_at" | "createdAt" => Ok(FieldPath::CreatedAt),
            "updated_at" | "updatedAt" => Ok(FieldPath::UpdatedAt),
            "scope_type" | "scopeType" => Ok(FieldPath::ScopeType),
            "scope_id" | "scopeId" => Ok(FieldPath::ScopeId),
            "workspace" => Ok(FieldPath::Workspace),
            "tier" => Ok(FieldPath::Tier),
            s if s.starts_with("metadata.") => {
                let json_path = s.strip_prefix("metadata.").unwrap();
                // Validate the JSON path to prevent SQL injection
                validate_json_path(json_path)?;
                Ok(FieldPath::Metadata(json_path.to_string()))
            }
            _ => Err(EngramError::InvalidInput(format!(
                "Unknown filter field: {}. Valid fields: content, memory_type, importance, tags, created_at, updated_at, scope_type, scope_id, workspace, tier, metadata.*",
                path
            ))),
        }
    }

    /// Convert to SQL column reference
    pub fn to_sql_column(&self) -> String {
        match self {
            FieldPath::Content => "m.content".to_string(),
            FieldPath::MemoryType => "m.memory_type".to_string(),
            FieldPath::Importance => "m.importance".to_string(),
            FieldPath::Tags => "m.id".to_string(), // Special handling for tags
            FieldPath::CreatedAt => "m.created_at".to_string(),
            FieldPath::UpdatedAt => "m.updated_at".to_string(),
            FieldPath::ScopeType => "m.scope_type".to_string(),
            FieldPath::ScopeId => "m.scope_id".to_string(),
            FieldPath::Workspace => "m.workspace".to_string(),
            FieldPath::Tier => "m.tier".to_string(),
            FieldPath::Metadata(path) => {
                // Convert dot notation to SQLite JSON path
                // e.g., "project.name" -> "$.project.name"
                // Note: path has been validated in parse() to contain only safe characters
                format!("json_extract(m.metadata, '$.{}')", path)
            }
        }
    }
}

/// SQL generation context for building parameterized queries
pub struct SqlBuilder {
    params: Vec<Box<dyn rusqlite::ToSql>>,
}

impl SqlBuilder {
    pub fn new() -> Self {
        Self { params: Vec::new() }
    }

    /// Build SQL from a filter expression
    pub fn build_filter(&mut self, expr: &FilterExpr) -> Result<String> {
        match expr {
            FilterExpr::And { conditions } => {
                if conditions.is_empty() {
                    return Ok("1=1".to_string()); // Always true
                }
                let parts: Result<Vec<String>> =
                    conditions.iter().map(|c| self.build_filter(c)).collect();
                Ok(format!("({})", parts?.join(" AND ")))
            }
            FilterExpr::Or { conditions } => {
                if conditions.is_empty() {
                    return Ok("1=0".to_string()); // Always false
                }
                let parts: Result<Vec<String>> =
                    conditions.iter().map(|c| self.build_filter(c)).collect();
                Ok(format!("({})", parts?.join(" OR ")))
            }
            FilterExpr::Condition(field_condition) => self.build_field_condition(field_condition),
        }
    }

    fn build_field_condition(&mut self, condition: &FieldCondition) -> Result<String> {
        // Validate: empty conditions are not allowed
        if condition.inner.is_empty() {
            return Err(EngramError::InvalidInput(
                "Empty filter condition. Each condition must specify at least one field."
                    .to_string(),
            ));
        }

        let mut parts = Vec::new();

        for (field_path, op) in &condition.inner {
            let field = FieldPath::parse(field_path)?;
            let sql = self.build_op(&field, op)?;
            parts.push(sql);
        }

        if parts.len() == 1 {
            Ok(parts.remove(0))
        } else {
            Ok(format!("({})", parts.join(" AND ")))
        }
    }

    fn build_op(&mut self, field: &FieldPath, op: &FilterOp) -> Result<String> {
        let column = field.to_sql_column();

        match (field, op) {
            // Special handling for tags (array field stored in separate table)
            // All tag operations must use EXISTS subqueries
            (FieldPath::Tags, FilterOp::Contains(value))
            | (FieldPath::Tags, FilterOp::Eq(value))
            | (FieldPath::Tags, FilterOp::Direct(value)) => {
                // For tags, eq/direct/contains all mean "has this tag"
                let tag = value.as_str().ok_or_else(|| {
                    EngramError::InvalidInput(
                        "tags filter requires a string value".to_string(),
                    )
                })?;
                self.params.push(Box::new(tag.to_string()));
                Ok(
                    "EXISTS (SELECT 1 FROM memory_tags mt JOIN tags t ON mt.tag_id = t.id WHERE mt.memory_id = m.id AND t.name = ?)".to_string()
                )
            }
            (FieldPath::Tags, FilterOp::NotContains(value))
            | (FieldPath::Tags, FilterOp::Neq(value)) => {
                // For tags, neq/not_contains mean "does not have this tag"
                let tag = value.as_str().ok_or_else(|| {
                    EngramError::InvalidInput(
                        "tags filter requires a string value".to_string(),
                    )
                })?;
                self.params.push(Box::new(tag.to_string()));
                Ok(
                    "NOT EXISTS (SELECT 1 FROM memory_tags mt JOIN tags t ON mt.tag_id = t.id WHERE mt.memory_id = m.id AND t.name = ?)".to_string()
                )
            }
            (FieldPath::Tags, FilterOp::Exists(exists)) => {
                // Check if memory has any tags at all
                if *exists {
                    Ok(
                        "EXISTS (SELECT 1 FROM memory_tags mt WHERE mt.memory_id = m.id)".to_string()
                    )
                } else {
                    Ok(
                        "NOT EXISTS (SELECT 1 FROM memory_tags mt WHERE mt.memory_id = m.id)".to_string()
                    )
                }
            }
            (FieldPath::Tags, FilterOp::Gt(_))
            | (FieldPath::Tags, FilterOp::Gte(_))
            | (FieldPath::Tags, FilterOp::Lt(_))
            | (FieldPath::Tags, FilterOp::Lte(_)) => {
                Err(EngramError::InvalidInput(
                    "Comparison operators (gt, gte, lt, lte) are not supported for tags. Use contains, eq, neq, or exists.".to_string(),
                ))
            }
            // Regular field operations
            (_, FilterOp::Eq(value)) | (_, FilterOp::Direct(value)) => {
                if value.is_null() {
                    Ok(format!("{} IS NULL", column))
                } else {
                    self.push_value_param(value)?;
                    Ok(format!("{} = ?", column))
                }
            }
            (_, FilterOp::Neq(value)) => {
                if value.is_null() {
                    Ok(format!("{} IS NOT NULL", column))
                } else {
                    self.push_value_param(value)?;
                    Ok(format!("{} != ?", column))
                }
            }
            (_, FilterOp::Gt(value)) => {
                self.push_value_param(value)?;
                Ok(format!("{} > ?", column))
            }
            (_, FilterOp::Gte(value)) => {
                self.push_value_param(value)?;
                Ok(format!("{} >= ?", column))
            }
            (_, FilterOp::Lt(value)) => {
                self.push_value_param(value)?;
                Ok(format!("{} < ?", column))
            }
            (_, FilterOp::Lte(value)) => {
                self.push_value_param(value)?;
                Ok(format!("{} <= ?", column))
            }
            (_, FilterOp::Contains(value)) => {
                // For non-tags fields, use LIKE for string contains
                let s = value.as_str().ok_or_else(|| {
                    EngramError::InvalidInput("contains requires a string value".to_string())
                })?;
                self.params.push(Box::new(format!("%{}%", s)));
                Ok(format!("{} LIKE ?", column))
            }
            (_, FilterOp::NotContains(value)) => {
                let s = value.as_str().ok_or_else(|| {
                    EngramError::InvalidInput("not_contains requires a string value".to_string())
                })?;
                self.params.push(Box::new(format!("%{}%", s)));
                Ok(format!("{} NOT LIKE ?", column))
            }
            (_, FilterOp::Exists(exists)) => {
                if *exists {
                    Ok(format!("{} IS NOT NULL", column))
                } else {
                    Ok(format!("{} IS NULL", column))
                }
            }
        }
    }

    fn push_value_param(&mut self, value: &Value) -> Result<()> {
        match value {
            Value::String(s) => {
                self.params.push(Box::new(s.clone()));
            }
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    self.params.push(Box::new(i));
                } else if let Some(f) = n.as_f64() {
                    self.params.push(Box::new(f));
                } else {
                    return Err(EngramError::InvalidInput(
                        "Invalid number value".to_string(),
                    ));
                }
            }
            Value::Bool(b) => {
                self.params.push(Box::new(*b));
            }
            Value::Null => {
                // Handled specially in operators
            }
            _ => {
                return Err(EngramError::InvalidInput(format!(
                    "Unsupported filter value type: {:?}",
                    value
                )));
            }
        }
        Ok(())
    }

    /// Take the accumulated parameters
    pub fn take_params(&mut self) -> Vec<Box<dyn rusqlite::ToSql>> {
        std::mem::take(&mut self.params)
    }
}

impl Default for SqlBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate that a JSON object doesn't mix AND/OR with field conditions.
/// This prevents silent data loss from serde's untagged enum parsing.
fn validate_no_mixed_keys(obj: &serde_json::Map<String, Value>) -> Result<()> {
    let has_and = obj.contains_key("AND");
    let has_or = obj.contains_key("OR");
    let has_other_keys = obj.keys().any(|k| k != "AND" && k != "OR");

    if (has_and || has_or) && has_other_keys {
        let logical_key = if has_and { "AND" } else { "OR" };
        let other_keys: Vec<_> = obj.keys().filter(|k| *k != "AND" && *k != "OR").collect();
        return Err(EngramError::InvalidInput(format!(
            "Filter object cannot mix '{}' with field conditions {:?}. \
             Use nested AND/OR to combine logical and field conditions.",
            logical_key, other_keys
        )));
    }

    // Recursively validate nested structures
    for (key, value) in obj {
        if key == "AND" || key == "OR" {
            if let Some(arr) = value.as_array() {
                for item in arr {
                    if let Some(nested_obj) = item.as_object() {
                        validate_no_mixed_keys(nested_obj)?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Parse a filter expression from JSON
pub fn parse_filter(json: &Value) -> Result<FilterExpr> {
    // Validate no mixed AND/OR with field conditions before parsing
    if let Some(obj) = json.as_object() {
        validate_no_mixed_keys(obj)?;
    }

    serde_json::from_value(json.clone())
        .map_err(|e| EngramError::InvalidInput(format!("Invalid filter syntax: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_simple_eq() {
        let json = json!({"metadata.project": {"eq": "engram"}});
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("json_extract(m.metadata, '$.project') = ?"));
    }

    #[test]
    fn test_parse_and_conditions() {
        let json = json!({
            "AND": [
                {"metadata.project": {"eq": "engram"}},
                {"metadata.priority": {"gte": 3}}
            ]
        });
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("AND"));
        assert!(sql.contains("json_extract(m.metadata, '$.project') = ?"));
        assert!(sql.contains("json_extract(m.metadata, '$.priority') >= ?"));
    }

    #[test]
    fn test_parse_or_conditions() {
        let json = json!({
            "OR": [
                {"tags": {"contains": "rust"}},
                {"tags": {"contains": "performance"}}
            ]
        });
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("OR"));
        assert!(sql.contains("EXISTS"));
    }

    #[test]
    fn test_parse_nested_and_or() {
        let json = json!({
            "AND": [
                {"metadata.project": {"eq": "engram"}},
                {"OR": [
                    {"tags": {"contains": "rust"}},
                    {"importance": {"gte": 0.8}}
                ]}
            ]
        });
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("AND"));
        assert!(sql.contains("OR"));
    }

    #[test]
    fn test_comparison_operators() {
        let cases = vec![
            (json!({"importance": {"gt": 0.5}}), ">"),
            (json!({"importance": {"gte": 0.5}}), ">="),
            (json!({"importance": {"lt": 0.5}}), "<"),
            (json!({"importance": {"lte": 0.5}}), "<="),
            (json!({"importance": {"neq": 0.5}}), "!="),
        ];

        for (json, expected_op) in cases {
            let filter = parse_filter(&json).unwrap();
            let mut builder = SqlBuilder::new();
            let sql = builder.build_filter(&filter).unwrap();
            assert!(
                sql.contains(expected_op),
                "Expected '{}' in SQL: {}",
                expected_op,
                sql
            );
        }
    }

    #[test]
    fn test_exists_operator() {
        let json = json!({"metadata.optional_field": {"exists": true}});
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("IS NOT NULL"));
    }

    #[test]
    fn test_not_exists_operator() {
        let json = json!({"metadata.optional_field": {"exists": false}});
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("IS NULL"));
    }

    #[test]
    fn test_string_contains() {
        let json = json!({"content": {"contains": "important"}});
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("LIKE"));
    }

    #[test]
    fn test_backwards_compatible_direct_value() {
        // Old style: {"metadata.key": "value"} should work as equality
        let json = json!({"metadata.status": "active"});
        let filter = parse_filter(&json).unwrap();

        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();

        assert!(sql.contains("= ?"));
    }

    #[test]
    fn test_field_path_parsing() {
        assert!(matches!(
            FieldPath::parse("content"),
            Ok(FieldPath::Content)
        ));
        assert!(matches!(
            FieldPath::parse("memory_type"),
            Ok(FieldPath::MemoryType)
        ));
        assert!(matches!(
            FieldPath::parse("type"),
            Ok(FieldPath::MemoryType)
        ));
        assert!(matches!(FieldPath::parse("tags"), Ok(FieldPath::Tags)));
        assert!(matches!(
            FieldPath::parse("metadata.project"),
            Ok(FieldPath::Metadata(ref s)) if s == "project"
        ));
        assert!(matches!(
            FieldPath::parse("metadata.config.timeout"),
            Ok(FieldPath::Metadata(ref s)) if s == "config.timeout"
        ));
        assert!(FieldPath::parse("invalid_field").is_err());
    }

    // Security tests for SQL injection prevention
    #[test]
    fn test_sql_injection_prevention_quotes() {
        // Attempt to break out of JSON path with quotes
        let result = FieldPath::parse("metadata.foo'); DROP TABLE memories; --");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid character"));
    }

    #[test]
    fn test_sql_injection_prevention_brackets() {
        // Attempt injection with brackets
        let result = FieldPath::parse("metadata.foo[0]");
        assert!(result.is_err());
    }

    #[test]
    fn test_sql_injection_prevention_semicolon() {
        let result = FieldPath::parse("metadata.foo; DELETE FROM memories");
        assert!(result.is_err());
    }

    #[test]
    fn test_sql_injection_prevention_backslash() {
        let result = FieldPath::parse("metadata.foo\\bar");
        assert!(result.is_err());
    }

    #[test]
    fn test_metadata_path_valid_characters() {
        // Valid paths should work
        assert!(FieldPath::parse("metadata.project_name").is_ok());
        assert!(FieldPath::parse("metadata.config-key").is_ok());
        assert!(FieldPath::parse("metadata.nested.path.here").is_ok());
        assert!(FieldPath::parse("metadata.CamelCase123").is_ok());
    }

    #[test]
    fn test_metadata_path_empty() {
        // Empty path after metadata. should fail
        let result = FieldPath::parse("metadata.");
        assert!(result.is_err());
    }

    #[test]
    fn test_metadata_path_double_dot() {
        // Path traversal attempt should fail
        let result = FieldPath::parse("metadata.foo..bar");
        assert!(result.is_err());
    }

    // Empty condition validation tests
    #[test]
    fn test_empty_condition_rejected() {
        let json = json!({});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let result = builder.build_filter(&filter);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Empty filter condition"));
    }

    #[test]
    fn test_empty_and_array_produces_true() {
        // Empty AND should be always true (identity for AND)
        let json = json!({"AND": []});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();
        assert_eq!(sql, "1=1");
    }

    #[test]
    fn test_empty_or_array_produces_false() {
        // Empty OR should be always false (identity for OR)
        let json = json!({"OR": []});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();
        assert_eq!(sql, "1=0");
    }

    // Tag filter semantics tests
    #[test]
    fn test_tags_eq_uses_exists() {
        // {"tags": {"eq": "rust"}} should use EXISTS subquery
        let json = json!({"tags": {"eq": "rust"}});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();
        assert!(sql.contains("EXISTS"));
        assert!(sql.contains("memory_tags"));
    }

    #[test]
    fn test_tags_direct_value_uses_exists() {
        // {"tags": "rust"} should use EXISTS subquery
        let json = json!({"tags": "rust"});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();
        assert!(sql.contains("EXISTS"));
    }

    #[test]
    fn test_tags_neq_uses_not_exists() {
        // {"tags": {"neq": "deprecated"}} should use NOT EXISTS
        let json = json!({"tags": {"neq": "deprecated"}});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();
        assert!(sql.contains("NOT EXISTS"));
    }

    #[test]
    fn test_tags_exists_true() {
        // {"tags": {"exists": true}} should check if memory has any tags
        let json = json!({"tags": {"exists": true}});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();
        assert!(sql.contains("EXISTS"));
        assert!(sql.contains("memory_tags"));
        // Should NOT join with tags table (just check memory_tags)
        assert!(!sql.contains("t.name"));
    }

    #[test]
    fn test_tags_exists_false() {
        // {"tags": {"exists": false}} should check if memory has no tags
        let json = json!({"tags": {"exists": false}});
        let filter = parse_filter(&json).unwrap();
        let mut builder = SqlBuilder::new();
        let sql = builder.build_filter(&filter).unwrap();
        assert!(sql.contains("NOT EXISTS"));
    }

    #[test]
    fn test_tags_comparison_operators_rejected() {
        // Comparison operators don't make sense for tags
        let cases = vec![
            json!({"tags": {"gt": "rust"}}),
            json!({"tags": {"gte": "rust"}}),
            json!({"tags": {"lt": "rust"}}),
            json!({"tags": {"lte": "rust"}}),
        ];

        for json in cases {
            let filter = parse_filter(&json).unwrap();
            let mut builder = SqlBuilder::new();
            let result = builder.build_filter(&filter);
            assert!(result.is_err(), "Expected error for: {:?}", json);
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("not supported for tags"));
        }
    }

    // Mixed AND/OR with field conditions tests (P2 fix)
    #[test]
    fn test_mixed_and_with_field_rejected() {
        // {"AND": [...], "metadata.project": {...}} should be rejected
        let json = json!({
            "AND": [{"metadata.status": {"eq": "active"}}],
            "metadata.project": {"eq": "engram"}
        });
        let result = parse_filter(&json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cannot mix"));
        assert!(err.contains("AND"));
    }

    #[test]
    fn test_mixed_or_with_field_rejected() {
        // {"OR": [...], "tags": "rust"} should be rejected
        let json = json!({
            "OR": [{"metadata.status": {"eq": "active"}}],
            "tags": "rust"
        });
        let result = parse_filter(&json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cannot mix"));
        assert!(err.contains("OR"));
    }

    #[test]
    fn test_mixed_nested_rejected() {
        // Nested mixed should also be rejected
        let json = json!({
            "AND": [
                {
                    "OR": [{"tags": "rust"}],
                    "metadata.project": {"eq": "engram"}
                }
            ]
        });
        let result = parse_filter(&json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cannot mix"));
    }

    #[test]
    fn test_pure_and_accepted() {
        // Pure AND without extra keys should work
        let json = json!({
            "AND": [
                {"metadata.project": {"eq": "engram"}},
                {"tags": "rust"}
            ]
        });
        let result = parse_filter(&json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_pure_or_accepted() {
        // Pure OR without extra keys should work
        let json = json!({
            "OR": [
                {"metadata.project": {"eq": "engram"}},
                {"tags": "rust"}
            ]
        });
        let result = parse_filter(&json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nested_and_or_accepted() {
        // Proper nesting should work
        let json = json!({
            "AND": [
                {"metadata.project": {"eq": "engram"}},
                {
                    "OR": [
                        {"tags": "rust"},
                        {"tags": "performance"}
                    ]
                }
            ]
        });
        let result = parse_filter(&json);
        assert!(result.is_ok());
    }
}
