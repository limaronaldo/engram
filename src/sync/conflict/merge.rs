//! Three-way merge implementation

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Result of a three-way merge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeResult {
    /// The merged content
    pub content: String,
    /// Whether the merge was successful (no conflicts)
    pub success: bool,
    /// Conflict markers in the output (if any)
    pub has_conflict_markers: bool,
    /// Lines that had conflicts
    pub conflict_lines: Vec<usize>,
    /// Statistics about the merge
    pub stats: MergeStats,
}

/// Statistics about a merge operation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MergeStats {
    /// Lines from base that were kept
    pub base_kept: usize,
    /// Lines added by local
    pub local_added: usize,
    /// Lines added by remote
    pub remote_added: usize,
    /// Lines deleted by local
    pub local_deleted: usize,
    /// Lines deleted by remote
    pub remote_deleted: usize,
    /// Lines with conflicts
    pub conflicts: usize,
}

/// Three-way merge implementation
pub struct ThreeWayMerge {
    /// Marker for local changes in conflict
    local_marker: String,
    /// Marker for remote changes in conflict
    remote_marker: String,
    /// Separator between versions
    separator: String,
}

impl Default for ThreeWayMerge {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreeWayMerge {
    /// Create a new three-way merger
    pub fn new() -> Self {
        Self {
            local_marker: "<<<<<<< LOCAL".to_string(),
            remote_marker: ">>>>>>> REMOTE".to_string(),
            separator: "=======".to_string(),
        }
    }

    /// Set custom conflict markers
    pub fn with_markers(
        mut self,
        local: impl Into<String>,
        separator: impl Into<String>,
        remote: impl Into<String>,
    ) -> Self {
        self.local_marker = local.into();
        self.separator = separator.into();
        self.remote_marker = remote.into();
        self
    }

    /// Perform three-way merge
    pub fn merge(&self, base: &str, local: &str, remote: &str) -> MergeResult {
        let base_lines: Vec<&str> = base.lines().collect();
        let local_lines: Vec<&str> = local.lines().collect();
        let remote_lines: Vec<&str> = remote.lines().collect();

        let mut result = Vec::new();
        let mut stats = MergeStats::default();
        let mut conflict_lines = Vec::new();
        let mut has_conflicts = false;

        let max_len = base_lines
            .len()
            .max(local_lines.len())
            .max(remote_lines.len());
        let mut base_idx = 0;
        let mut local_idx = 0;
        let mut remote_idx = 0;

        while base_idx < base_lines.len()
            || local_idx < local_lines.len()
            || remote_idx < remote_lines.len()
        {
            let base_line = base_lines.get(base_idx);
            let local_line = local_lines.get(local_idx);
            let remote_line = remote_lines.get(remote_idx);

            match (base_line, local_line, remote_line) {
                // All three match - keep it
                (Some(b), Some(l), Some(r)) if b == l && l == r => {
                    result.push((*l).to_string());
                    stats.base_kept += 1;
                    base_idx += 1;
                    local_idx += 1;
                    remote_idx += 1;
                }

                // Local changed, remote unchanged - take local
                (Some(b), Some(l), Some(r)) if b == r && b != l => {
                    result.push((*l).to_string());
                    stats.local_added += 1;
                    base_idx += 1;
                    local_idx += 1;
                    remote_idx += 1;
                }

                // Remote changed, local unchanged - take remote
                (Some(b), Some(l), Some(r)) if b == l && b != r => {
                    result.push((*r).to_string());
                    stats.remote_added += 1;
                    base_idx += 1;
                    local_idx += 1;
                    remote_idx += 1;
                }

                // Both changed differently - conflict!
                (Some(_), Some(l), Some(r)) if l != r => {
                    has_conflicts = true;
                    stats.conflicts += 1;
                    conflict_lines.push(result.len());

                    result.push(self.local_marker.clone());
                    result.push((*l).to_string());
                    result.push(self.separator.clone());
                    result.push((*r).to_string());
                    result.push(self.remote_marker.clone());

                    base_idx += 1;
                    local_idx += 1;
                    remote_idx += 1;
                }

                // Both changed to same - take it
                (Some(_), Some(l), Some(r)) if l == r => {
                    result.push((*l).to_string());
                    stats.local_added += 1;
                    base_idx += 1;
                    local_idx += 1;
                    remote_idx += 1;
                }

                // Local added line (past base)
                (None, Some(l), None) => {
                    result.push((*l).to_string());
                    stats.local_added += 1;
                    local_idx += 1;
                }

                // Remote added line (past base)
                (None, None, Some(r)) => {
                    result.push((*r).to_string());
                    stats.remote_added += 1;
                    remote_idx += 1;
                }

                // Both added different lines - conflict
                (None, Some(l), Some(r)) if l != r => {
                    has_conflicts = true;
                    stats.conflicts += 1;
                    conflict_lines.push(result.len());

                    result.push(self.local_marker.clone());
                    result.push((*l).to_string());
                    result.push(self.separator.clone());
                    result.push((*r).to_string());
                    result.push(self.remote_marker.clone());

                    local_idx += 1;
                    remote_idx += 1;
                }

                // Both added same line
                (None, Some(l), Some(r)) if l == r => {
                    result.push((*l).to_string());
                    stats.local_added += 1;
                    local_idx += 1;
                    remote_idx += 1;
                }

                // Local deleted (remote unchanged)
                (Some(_), None, Some(r)) if base_lines.get(base_idx) == Some(r) => {
                    stats.local_deleted += 1;
                    base_idx += 1;
                    remote_idx += 1;
                }

                // Remote deleted (local unchanged)
                (Some(_), Some(l), None) if base_lines.get(base_idx) == Some(l) => {
                    stats.remote_deleted += 1;
                    base_idx += 1;
                    local_idx += 1;
                }

                // Fallback: take what we have
                _ => {
                    if let Some(l) = local_line {
                        result.push((*l).to_string());
                    }
                    if let Some(r) = remote_line {
                        if local_line.map(|l| l != r).unwrap_or(true) {
                            result.push((*r).to_string());
                        }
                    }
                    break; // Prevent infinite loop
                }
            }

            // Safety: prevent infinite loops
            if base_idx + local_idx + remote_idx > max_len * 3 + 10 {
                break;
            }
        }

        MergeResult {
            content: result.join("\n"),
            success: !has_conflicts,
            has_conflict_markers: has_conflicts,
            conflict_lines,
            stats,
        }
    }

    /// Merge tags by taking union
    pub fn merge_tags(&self, base: &[String], local: &[String], remote: &[String]) -> Vec<String> {
        let mut result: HashSet<String> = HashSet::new();

        // Start with base
        result.extend(base.iter().cloned());

        // Add local additions
        for tag in local {
            result.insert(tag.clone());
        }

        // Add remote additions
        for tag in remote {
            result.insert(tag.clone());
        }

        // Remove tags deleted by both
        let local_set: HashSet<_> = local.iter().collect();
        let remote_set: HashSet<_> = remote.iter().collect();

        result
            .into_iter()
            .filter(|tag| {
                // Keep if: in local OR in remote OR (in base AND (in local OR in remote))
                local_set.contains(tag) || remote_set.contains(tag)
            })
            .collect()
    }

    /// Merge metadata by combining with local preference for conflicts
    /// Now works with HashMap directly
    pub fn merge_metadata_map(
        &self,
        base: Option<&HashMap<String, serde_json::Value>>,
        local: &HashMap<String, serde_json::Value>,
        remote: &HashMap<String, serde_json::Value>,
    ) -> HashMap<String, serde_json::Value> {
        let mut result = HashMap::new();

        // If local and remote are the same, return local
        if local == remote {
            return local.clone();
        }

        // Start with base if present, otherwise start fresh
        if let Some(base_map) = base {
            result = base_map.clone();
        }

        // Apply local changes
        for (k, v) in local {
            let base_value = base.and_then(|b| b.get(k));
            if base_value != Some(v) {
                result.insert(k.clone(), v.clone());
            }
        }

        // Apply remote changes (if not conflicting with local)
        for (k, v) in remote {
            let base_value = base.and_then(|b| b.get(k));
            let local_value = local.get(k);
            // Only apply remote change if local didn't change this key from base
            if base_value != Some(v) && local_value == base_value {
                result.insert(k.clone(), v.clone());
            }
        }

        // Add any keys that are only in local or remote (not in base)
        for (k, v) in local {
            if !result.contains_key(k) {
                result.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in remote {
            if !result.contains_key(k) {
                result.insert(k.clone(), v.clone());
            }
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_no_conflict() {
        let merger = ThreeWayMerge::new();

        let base = "Line 1\nLine 2\nLine 3";
        let local = "Line 1 modified\nLine 2\nLine 3";
        let remote = "Line 1\nLine 2\nLine 3 modified";

        let result = merger.merge(base, local, remote);
        assert!(result.success);
        assert!(!result.has_conflict_markers);
        assert!(result.content.contains("Line 1 modified"));
        assert!(result.content.contains("Line 3 modified"));
    }

    #[test]
    fn test_merge_with_conflict() {
        let merger = ThreeWayMerge::new();

        let base = "Line 1\nLine 2";
        let local = "Local change\nLine 2";
        let remote = "Remote change\nLine 2";

        let result = merger.merge(base, local, remote);
        assert!(!result.success);
        assert!(result.has_conflict_markers);
        assert!(result.content.contains("<<<<<<< LOCAL"));
        assert!(result.content.contains("Local change"));
        assert!(result.content.contains("======="));
        assert!(result.content.contains("Remote change"));
        assert!(result.content.contains(">>>>>>> REMOTE"));
    }

    #[test]
    fn test_merge_both_same_change() {
        let merger = ThreeWayMerge::new();

        let base = "Original";
        let local = "Same change";
        let remote = "Same change";

        let result = merger.merge(base, local, remote);
        assert!(result.success);
        assert_eq!(result.content, "Same change");
    }

    #[test]
    fn test_merge_tags() {
        let merger = ThreeWayMerge::new();

        let base = vec!["tag1".to_string(), "tag2".to_string()];
        let local = vec![
            "tag1".to_string(),
            "tag2".to_string(),
            "local_tag".to_string(),
        ];
        let remote = vec![
            "tag1".to_string(),
            "tag2".to_string(),
            "remote_tag".to_string(),
        ];

        let result = merger.merge_tags(&base, &local, &remote);
        assert!(result.contains(&"tag1".to_string()));
        assert!(result.contains(&"tag2".to_string()));
        assert!(result.contains(&"local_tag".to_string()));
        assert!(result.contains(&"remote_tag".to_string()));
    }

    #[test]
    fn test_merge_metadata_map() {
        let merger = ThreeWayMerge::new();

        let mut base = HashMap::new();
        base.insert("a".to_string(), serde_json::json!(1));
        base.insert("b".to_string(), serde_json::json!(2));

        let mut local = HashMap::new();
        local.insert("a".to_string(), serde_json::json!(10)); // Changed a
        local.insert("b".to_string(), serde_json::json!(2));

        let mut remote = HashMap::new();
        remote.insert("a".to_string(), serde_json::json!(1));
        remote.insert("b".to_string(), serde_json::json!(20)); // Changed b

        let result = merger.merge_metadata_map(Some(&base), &local, &remote);
        assert_eq!(result.get("a"), Some(&serde_json::json!(10))); // Local's change
        assert_eq!(result.get("b"), Some(&serde_json::json!(20))); // Remote's change
    }
}
