//! Advanced metadata query syntax (RML-879)
//!
//! Supports MongoDB-style operators for flexible metadata filtering.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{EngramError, Result};

/// Metadata query with operators
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetadataQuery {
    /// Simple equality: {"key": "value"}
    Eq(Value),
    /// Operators: {"key": {"$gt": 5}}
    Operators(QueryOperators),
}

/// Query operators
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QueryOperators {
    #[serde(rename = "$eq")]
    pub eq: Option<Value>,
    #[serde(rename = "$ne")]
    pub ne: Option<Value>,
    #[serde(rename = "$gt")]
    pub gt: Option<Value>,
    #[serde(rename = "$gte")]
    pub gte: Option<Value>,
    #[serde(rename = "$lt")]
    pub lt: Option<Value>,
    #[serde(rename = "$lte")]
    pub lte: Option<Value>,
    #[serde(rename = "$in")]
    pub r#in: Option<Vec<Value>>,
    #[serde(rename = "$nin")]
    pub nin: Option<Vec<Value>>,
    #[serde(rename = "$contains")]
    pub contains: Option<Value>,
    #[serde(rename = "$exists")]
    pub exists: Option<bool>,
    #[serde(rename = "$regex")]
    pub regex: Option<String>,
}

/// Parse a metadata filter into SQL WHERE clauses
pub fn parse_metadata_filter(
    filter: &serde_json::Map<String, Value>,
) -> Result<(String, Vec<Box<dyn rusqlite::ToSql + Send>>)> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::new();

    for (key, value) in filter {
        let json_path = if key.contains('.') {
            // Nested path: metadata.config.timeout -> $.config.timeout
            format!("$.{}", key.replace("metadata.", ""))
        } else {
            format!("$.{}", key)
        };

        match value {
            // Simple equality
            Value::String(s) => {
                conditions.push(format!("json_extract(metadata, '{}') = ?", json_path));
                params.push(Box::new(s.clone()));
            }
            Value::Number(n) => {
                conditions.push(format!("json_extract(metadata, '{}') = ?", json_path));
                if let Some(i) = n.as_i64() {
                    params.push(Box::new(i));
                } else if let Some(f) = n.as_f64() {
                    params.push(Box::new(f));
                }
            }
            Value::Bool(b) => {
                conditions.push(format!("json_extract(metadata, '{}') = ?", json_path));
                params.push(Box::new(*b));
            }
            // Operators
            Value::Object(ops) => {
                let (op_conditions, op_params) = parse_operators(&json_path, ops)?;
                conditions.extend(op_conditions);
                params.extend(op_params);
            }
            _ => {
                return Err(EngramError::InvalidInput(format!(
                    "Unsupported filter value type for key: {}",
                    key
                )));
            }
        }
    }

    let sql = if conditions.is_empty() {
        "1=1".to_string()
    } else {
        conditions.join(" AND ")
    };

    Ok((sql, params))
}

/// Parse operator object into SQL conditions
fn parse_operators(
    json_path: &str,
    ops: &serde_json::Map<String, Value>,
) -> Result<(Vec<String>, Vec<Box<dyn rusqlite::ToSql + Send>>)> {
    let mut conditions: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::new();

    for (op, value) in ops {
        match op.as_str() {
            "$eq" => {
                conditions.push(format!("json_extract(metadata, '{}') = ?", json_path));
                params.push(value_to_param(value)?);
            }
            "$ne" => {
                conditions.push(format!(
                    "(json_extract(metadata, '{}') IS NULL OR json_extract(metadata, '{}') != ?)",
                    json_path, json_path
                ));
                params.push(value_to_param(value)?);
            }
            "$gt" => {
                conditions.push(format!("json_extract(metadata, '{}') > ?", json_path));
                params.push(value_to_param(value)?);
            }
            "$gte" => {
                conditions.push(format!("json_extract(metadata, '{}') >= ?", json_path));
                params.push(value_to_param(value)?);
            }
            "$lt" => {
                conditions.push(format!("json_extract(metadata, '{}') < ?", json_path));
                params.push(value_to_param(value)?);
            }
            "$lte" => {
                conditions.push(format!("json_extract(metadata, '{}') <= ?", json_path));
                params.push(value_to_param(value)?);
            }
            "$in" => {
                if let Value::Array(arr) = value {
                    if arr.is_empty() {
                        conditions.push("0=1".to_string());
                    } else {
                        let placeholders: Vec<&str> = arr.iter().map(|_| "?").collect();
                        conditions.push(format!(
                            "json_extract(metadata, '{}') IN ({})",
                            json_path,
                            placeholders.join(", ")
                        ));
                        for v in arr {
                            params.push(value_to_param(v)?);
                        }
                    }
                } else {
                    return Err(EngramError::InvalidInput(
                        "$in operator requires an array".to_string(),
                    ));
                }
            }
            "$nin" => {
                if let Value::Array(arr) = value {
                    if arr.is_empty() {
                        conditions.push("1=1".to_string());
                    } else {
                        let placeholders: Vec<&str> = arr.iter().map(|_| "?").collect();
                        conditions.push(format!(
                            "(json_extract(metadata, '{}') IS NULL OR json_extract(metadata, '{}') NOT IN ({}))",
                            json_path,
                            json_path,
                            placeholders.join(", ")
                        ));
                        for v in arr {
                            params.push(value_to_param(v)?);
                        }
                    }
                } else {
                    return Err(EngramError::InvalidInput(
                        "$nin operator requires an array".to_string(),
                    ));
                }
            }
            "$contains" => {
                // For array contains or string contains
                if let Value::String(s) = value {
                    conditions.push(format!("json_extract(metadata, '{}') LIKE ?", json_path));
                    params.push(Box::new(format!("%{}%", s)));
                } else {
                    return Err(EngramError::InvalidInput(
                        "$contains operator requires a string".to_string(),
                    ));
                }
            }
            "$exists" => {
                if let Value::Bool(exists) = value {
                    if *exists {
                        conditions.push(format!(
                            "json_extract(metadata, '{}') IS NOT NULL",
                            json_path
                        ));
                    } else {
                        conditions.push(format!("json_extract(metadata, '{}') IS NULL", json_path));
                    }
                }
            }
            "$regex" => {
                if let Value::String(pattern) = value {
                    // SQLite doesn't have native regex, use GLOB or LIKE
                    // Convert basic regex to GLOB pattern
                    let glob_pattern = regex_to_glob(pattern);
                    conditions.push(format!("json_extract(metadata, '{}') GLOB ?", json_path));
                    params.push(Box::new(glob_pattern));
                } else {
                    return Err(EngramError::InvalidInput(
                        "$regex operator requires a string".to_string(),
                    ));
                }
            }
            _ => {
                return Err(EngramError::InvalidInput(format!(
                    "Unknown operator: {}",
                    op
                )));
            }
        }
    }

    Ok((conditions, params))
}

/// Convert a JSON value to a SQL parameter
fn value_to_param(value: &Value) -> Result<Box<dyn rusqlite::ToSql + Send>> {
    match value {
        Value::String(s) => Ok(Box::new(s.clone())),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Box::new(i))
            } else if let Some(f) = n.as_f64() {
                Ok(Box::new(f))
            } else {
                Err(EngramError::InvalidInput("Invalid number".to_string()))
            }
        }
        Value::Bool(b) => Ok(Box::new(*b)),
        _ => Err(EngramError::InvalidInput(format!(
            "Unsupported value type: {:?}",
            value
        ))),
    }
}

/// Convert basic regex to SQLite GLOB pattern
fn regex_to_glob(regex: &str) -> String {
    regex
        .replace(".*", "*")
        .replace(".+", "?*")
        .replace(".", "?")
        .replace("^", "")
        .replace("$", "")
}

/// Build a complete metadata filter query
pub fn build_metadata_query(
    base_query: &str,
    filter: &serde_json::Map<String, Value>,
) -> Result<(String, Vec<Box<dyn rusqlite::ToSql + Send>>)> {
    let (filter_sql, params) = parse_metadata_filter(filter)?;

    let sql = if filter_sql == "1=1" {
        base_query.to_string()
    } else {
        format!("{} AND {}", base_query, filter_sql)
    };

    Ok((sql, params))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_equality() {
        let filter: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"status": "active"})).unwrap();
        let (sql, params) = parse_metadata_filter(&filter).unwrap();
        assert!(sql.contains("json_extract"));
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_comparison_operators() {
        let filter: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"count": {"$gt": 5, "$lte": 100}})).unwrap();
        let (sql, params) = parse_metadata_filter(&filter).unwrap();
        assert!(sql.contains(">"));
        assert!(sql.contains("<="));
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_in_operator() {
        let filter: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"priority": {"$in": ["high", "critical"]}})).unwrap();
        let (sql, params) = parse_metadata_filter(&filter).unwrap();
        assert!(sql.contains("IN"));
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn test_empty_in_operator() {
        let filter: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"priority": {"$in": []}})).unwrap();
        let (sql, params) = parse_metadata_filter(&filter).unwrap();
        assert!(sql.contains("0=1"));
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_empty_nin_operator() {
        let filter: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"priority": {"$nin": []}})).unwrap();
        let (sql, params) = parse_metadata_filter(&filter).unwrap();
        assert!(sql.contains("1=1"));
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_exists_operator() {
        let filter: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"email": {"$exists": true}})).unwrap();
        let (sql, _) = parse_metadata_filter(&filter).unwrap();
        assert!(sql.contains("IS NOT NULL"));
    }

    #[test]
    fn test_nested_path() {
        let filter: serde_json::Map<String, Value> =
            serde_json::from_value(json!({"config.timeout": 30})).unwrap();
        let (sql, _) = parse_metadata_filter(&filter).unwrap();
        assert!(sql.contains("$.config.timeout"));
    }
}
