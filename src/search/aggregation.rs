//! Aggregation queries for memory statistics (RML-880)
//!
//! Supports grouping by tags, type, time periods with various metrics.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Aggregation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationResult {
    /// Group key
    pub group: String,
    /// Metrics for this group
    pub metrics: AggregationMetrics,
}

/// Metrics calculated per group
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregationMetrics {
    pub count: i64,
    pub avg_importance: Option<f32>,
    pub total_access_count: Option<i64>,
    pub oldest: Option<String>,
    pub newest: Option<String>,
}

/// Group by options
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Type,
    Tags,
    Month,
    Week,
    Visibility,
}

impl GroupBy {
    pub fn as_sql_expr(&self) -> &'static str {
        match self {
            GroupBy::Type => "memory_type",
            GroupBy::Tags => "t.name",
            GroupBy::Month => "strftime('%Y-%m', created_at)",
            GroupBy::Week => "strftime('%Y-W%W', created_at)",
            GroupBy::Visibility => "visibility",
        }
    }
}

/// Metrics to calculate
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    Count,
    AvgImportance,
    TotalAccessCount,
    DateRange,
}

/// Perform aggregation query
pub fn aggregate_memories(
    conn: &Connection,
    group_by: GroupBy,
    metrics: &[Metric],
) -> Result<Vec<AggregationResult>> {
    // Build SELECT clause
    let mut select_parts = vec![format!("{} as group_key", group_by.as_sql_expr())];

    for metric in metrics {
        match metric {
            Metric::Count => select_parts.push("COUNT(*) as cnt".to_string()),
            Metric::AvgImportance => select_parts.push("AVG(importance) as avg_imp".to_string()),
            Metric::TotalAccessCount => {
                select_parts.push("SUM(access_count) as total_access".to_string())
            }
            Metric::DateRange => {
                select_parts.push("MIN(created_at) as oldest".to_string());
                select_parts.push("MAX(created_at) as newest".to_string());
            }
        }
    }

    // Build FROM clause (join tags table if grouping by tags)
    let from_clause = if group_by == GroupBy::Tags {
        "memories m
         JOIN memory_tags mt ON m.id = mt.memory_id
         JOIN tags t ON mt.tag_id = t.id"
    } else {
        "memories m"
    };

    let sql = format!(
        "SELECT {} FROM {} WHERE m.valid_to IS NULL GROUP BY group_key ORDER BY cnt DESC",
        select_parts.join(", "),
        from_clause
    );

    let mut stmt = conn.prepare(&sql)?;
    let mut results = Vec::new();

    let rows = stmt.query_map([], |row| {
        let group: String = row.get("group_key")?;

        let count: i64 = row.get("cnt").unwrap_or(0);
        let avg_importance: Option<f64> = row.get("avg_imp").ok();
        let total_access: Option<i64> = row.get("total_access").ok();
        let oldest: Option<String> = row.get("oldest").ok();
        let newest: Option<String> = row.get("newest").ok();

        Ok(AggregationResult {
            group,
            metrics: AggregationMetrics {
                count,
                avg_importance: avg_importance.map(|f| f as f32),
                total_access_count: total_access,
                oldest,
                newest,
            },
        })
    })?;

    for row in rows {
        results.push(row?);
    }

    Ok(results)
}

/// Get tag distribution
pub fn get_tag_distribution(conn: &Connection, limit: i64) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT t.name, COUNT(*) as cnt
         FROM tags t
         JOIN memory_tags mt ON t.id = mt.tag_id
         JOIN memories m ON mt.memory_id = m.id
         WHERE m.valid_to IS NULL
         GROUP BY t.name
         ORDER BY cnt DESC
         LIMIT ?",
    )?;

    let results: Vec<(String, i64)> = stmt
        .query_map([limit], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Get type distribution
pub fn get_type_distribution(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT memory_type, COUNT(*) as cnt
         FROM memories
         WHERE valid_to IS NULL
         GROUP BY memory_type
         ORDER BY cnt DESC",
    )?;

    let results: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

/// Get memories created over time (for trend analysis)
pub fn get_creation_trend(
    conn: &Connection,
    interval: &str, // "day", "week", "month"
) -> Result<Vec<(String, i64)>> {
    let format_str = match interval {
        "day" => "%Y-%m-%d",
        "week" => "%Y-W%W",
        "month" => "%Y-%m",
        "year" => "%Y",
        _ => "%Y-%m-%d",
    };

    let sql = format!(
        "SELECT strftime('{}', created_at) as period, COUNT(*) as cnt
         FROM memories
         WHERE valid_to IS NULL
         GROUP BY period
         ORDER BY period DESC
         LIMIT 100",
        format_str
    );

    let mut stmt = conn.prepare(&sql)?;
    let results: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_by_sql_expr() {
        assert_eq!(GroupBy::Type.as_sql_expr(), "memory_type");
        assert_eq!(GroupBy::Tags.as_sql_expr(), "t.name");
        assert_eq!(
            GroupBy::Month.as_sql_expr(),
            "strftime('%Y-%m', created_at)"
        );
    }
}
