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
    /// Nested metadata field (e.g., "metadata.project" or "metadata.config.timeout")
    Metadata(String),
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
            s if s.starts_with("metadata.") => {
                let json_path = s.strip_prefix("metadata.").unwrap();
                Ok(FieldPath::Metadata(json_path.to_string()))
            }
            _ => Err(EngramError::InvalidInput(format!(
                "Unknown filter field: {}. Valid fields: content, memory_type, importance, tags, created_at, updated_at, scope_type, scope_id, metadata.*",
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
            FieldPath::Metadata(path) => {
                // Convert dot notation to SQLite JSON path
                // e.g., "project.name" -> "$.project.name"
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
            (FieldPath::Tags, FilterOp::Contains(value)) => {
                let tag = value.as_str().ok_or_else(|| {
                    EngramError::InvalidInput("tags.contains requires a string value".to_string())
                })?;
                self.params.push(Box::new(tag.to_string()));
                Ok(format!(
                    "EXISTS (SELECT 1 FROM memory_tags mt JOIN tags t ON mt.tag_id = t.id WHERE mt.memory_id = m.id AND t.name = ?)"
                ))
            }
            (FieldPath::Tags, FilterOp::NotContains(value)) => {
                let tag = value.as_str().ok_or_else(|| {
                    EngramError::InvalidInput(
                        "tags.not_contains requires a string value".to_string(),
                    )
                })?;
                self.params.push(Box::new(tag.to_string()));
                Ok(format!(
                    "NOT EXISTS (SELECT 1 FROM memory_tags mt JOIN tags t ON mt.tag_id = t.id WHERE mt.memory_id = m.id AND t.name = ?)"
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

/// Parse a filter expression from JSON
pub fn parse_filter(json: &Value) -> Result<FilterExpr> {
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
}
