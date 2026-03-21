//! Markdown export handler — human-readable memory export.
//!
//! Exports memories as Markdown files with YAML frontmatter and
//! wiki-style [[links]] for browsing and version control.
//! Inspired by Basic Memory's Markdown-first approach.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::HandlerContext;

/// Export a workspace as a directory of Markdown files.
///
/// Params:
/// - `workspace` (string, required) — workspace to export
/// - `output_dir` (string, optional) — output directory
///   (default: `./engram-export/{workspace}/`)
/// - `include_links` (bool, optional, default true) — include
///   [[wiki links]] to related memories
pub fn memory_export_markdown(ctx: &HandlerContext, params: Value) -> Value {
    let workspace = match params.get("workspace").and_then(|v| v.as_str()) {
        Some(w) => w.to_string(),
        None => return json!({"error": "workspace is required"}),
    };

    let default_dir = format!("./engram-export/{}", workspace);
    let output_dir = params
        .get("output_dir")
        .and_then(|v| v.as_str())
        .unwrap_or(&default_dir);
    let output_path = PathBuf::from(output_dir);

    let include_links = params
        .get("include_links")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    // 1. Query all memories in workspace
    let memories = match query_workspace_memories(ctx, &workspace) {
        Ok(m) => m,
        Err(e) => return json!({"error": format!("Failed to query memories: {}", e)}),
    };

    if memories.is_empty() {
        return json!({
            "error": format!("No memories found in workspace '{}'", workspace),
            "files_written": 0
        });
    }

    // 2. If include_links, query cross-references for all memory IDs
    let related_map: HashMap<i64, Vec<(i64, String)>> = if include_links {
        let memory_ids: Vec<i64> = memories
            .iter()
            .filter_map(|m| m.get("id").and_then(|v| v.as_i64()))
            .collect();
        build_related_map(ctx, &memory_ids)
    } else {
        HashMap::new()
    };

    // 3. Create directory structure and write files
    if let Err(e) = fs::create_dir_all(&output_path) {
        return json!({"error": format!("Failed to create output directory: {}", e)});
    }

    // First pass: compute filenames
    let id_to_filename: HashMap<i64, String> = memories
        .iter()
        .filter_map(|mem| {
            let id = mem.get("id").and_then(|v| v.as_i64())?;
            let content = mem.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let title = content.lines().next().unwrap_or("untitled");
            let sanitized = sanitize_filename(title);
            Some((id, format!("{}-{}", id, sanitized)))
        })
        .collect();

    let mut type_counts: HashMap<String, usize> = HashMap::new();
    for mem in &memories {
        let mem_type = mem
            .get("memory_type")
            .and_then(|v| v.as_str())
            .unwrap_or("note");
        *type_counts.entry(mem_type.to_string()).or_insert(0) += 1;
    }

    // Second pass: write files
    let mut files_written: usize = 0;
    for mem in &memories {
        let id = mem.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let mem_type = mem
            .get("memory_type")
            .and_then(|v| v.as_str())
            .unwrap_or("note");

        // Create type subdirectory
        let type_dir = output_path.join(pluralize_type(mem_type));
        if let Err(e) = fs::create_dir_all(&type_dir) {
            return json!({
                "error": format!("Failed to create directory {}: {}", type_dir.display(), e)
            });
        }

        let filename = id_to_filename
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("{}", id));
        let file_path = type_dir.join(format!("{}.md", filename));

        let md = format_memory_markdown(mem, include_links, &related_map, &id_to_filename);

        if let Err(e) = fs::write(&file_path, &md) {
            return json!({"error": format!("Failed to write {}: {}", file_path.display(), e)});
        }
        files_written += 1;
    }

    // 4. Write index.md
    let index_path = output_path.join("index.md");
    let index = build_index_markdown(&workspace, &memories, &type_counts, &id_to_filename);

    if let Err(e) = fs::write(&index_path, &index) {
        return json!({"error": format!("Failed to write index: {}", e)});
    }

    json!({
        "files_written": files_written + 1,
        "output_dir": output_path.to_string_lossy(),
        "index_path": index_path.to_string_lossy(),
        "memories_exported": memories.len(),
        "type_breakdown": type_counts
    })
}

/// Query all active memories in a workspace.
fn query_workspace_memories(
    ctx: &HandlerContext,
    workspace: &str,
) -> Result<Vec<Value>, crate::error::EngramError> {
    ctx.storage.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT m.id, m.content, m.memory_type, m.importance, m.workspace, m.tier,
                    m.created_at, m.updated_at,
                    (SELECT GROUP_CONCAT(t.name, ',')
                     FROM memory_tags mt
                     JOIN tags t ON mt.tag_id = t.id
                     WHERE mt.memory_id = m.id) as tags
             FROM memories m
             WHERE m.workspace = ?1
               AND COALESCE(m.lifecycle_state, 'active') != 'archived'
               AND m.valid_to IS NULL
             ORDER BY m.memory_type, m.created_at",
        )?;
        let rows = stmt.query_map(rusqlite::params![workspace], |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "content": row.get::<_, String>(1)?,
                "memory_type": row.get::<_, String>(2)?,
                "importance": row.get::<_, Option<f64>>(3)?,
                "workspace": row.get::<_, String>(4)?,
                "tier": row.get::<_, Option<String>>(5)?,
                "created_at": row.get::<_, String>(6)?,
                "updated_at": row.get::<_, Option<String>>(7)?,
                "tags": row.get::<_, Option<String>>(8)?
            }))
        })?;
        let memories: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
        Ok(memories)
    })
}

/// Build a map of memory_id -> [(related_id, relation_type)].
fn build_related_map(
    ctx: &HandlerContext,
    memory_ids: &[i64],
) -> HashMap<i64, Vec<(i64, String)>> {
    let mut map: HashMap<i64, Vec<(i64, String)>> = HashMap::new();

    for &id in memory_ids {
        if let Ok(related) = ctx.storage.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT to_id, relation_type FROM cross_references WHERE from_id = ?1
                 UNION ALL
                 SELECT from_id, relation_type FROM cross_references WHERE to_id = ?1",
            )?;
            let rows: Vec<(i64, String)> = stmt
                .query_map(rusqlite::params![id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        }) {
            if !related.is_empty() {
                map.insert(id, related);
            }
        }
    }

    map
}

/// Format a single memory as Markdown with YAML frontmatter.
fn format_memory_markdown(
    mem: &Value,
    include_links: bool,
    related_map: &HashMap<i64, Vec<(i64, String)>>,
    id_to_filename: &HashMap<i64, String>,
) -> String {
    let id = mem.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
    let mem_type = mem
        .get("memory_type")
        .and_then(|v| v.as_str())
        .unwrap_or("note");
    let content = mem
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tags_str = mem
        .get("tags")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let importance = mem.get("importance").and_then(|v| v.as_f64());
    let tier = mem
        .get("tier")
        .and_then(|v| v.as_str())
        .unwrap_or("permanent");
    let created = mem
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let updated = mem.get("updated_at").and_then(|v| v.as_str());

    let tags_vec = parse_tags(tags_str);

    let mut md = String::new();

    // YAML frontmatter
    md.push_str("---\n");
    md.push_str(&format!("id: {}\n", id));
    md.push_str(&format!("type: {}\n", mem_type));
    if !tags_vec.is_empty() {
        let tags_yaml: Vec<String> = tags_vec.iter().map(|t| format!("\"{}\"", t)).collect();
        md.push_str(&format!("tags: [{}]\n", tags_yaml.join(", ")));
    }
    if let Some(imp) = importance {
        md.push_str(&format!("importance: {:.2}\n", imp));
    }
    md.push_str(&format!("tier: {}\n", tier));
    md.push_str(&format!("created_at: \"{}\"\n", created));
    if let Some(upd) = updated {
        md.push_str(&format!("updated_at: \"{}\"\n", upd));
    }
    md.push_str("---\n\n");

    // Content
    md.push_str(content);
    md.push('\n');

    // Related memories as [[wiki links]]
    if include_links {
        if let Some(related) = related_map.get(&id) {
            if !related.is_empty() {
                md.push_str("\n## Related\n\n");
                for (related_id, relation_type) in related {
                    let linked_name = id_to_filename
                        .get(related_id)
                        .cloned()
                        .unwrap_or_else(|| format!("memory-{}", related_id));
                    md.push_str(&format!("- {} [[{}]]\n", relation_type, linked_name));
                }
            }
        }
    }

    md
}

/// Build an index.md with a summary table of all exported memories.
fn build_index_markdown(
    workspace: &str,
    memories: &[Value],
    type_counts: &HashMap<String, usize>,
    id_to_filename: &HashMap<i64, String>,
) -> String {
    let mut index = String::new();
    index.push_str(&format!("# {} -- Engram Export\n\n", workspace));
    index.push_str(&format!("**Total memories:** {}\n\n", memories.len()));
    index.push_str("## By Type\n\n");

    let mut sorted_types: Vec<_> = type_counts.iter().collect();
    sorted_types.sort_by(|a, b| b.1.cmp(a.1));
    for (mem_type, count) in &sorted_types {
        index.push_str(&format!(
            "- **{}/** -- {} memories\n",
            pluralize_type(mem_type),
            count
        ));
    }

    index.push_str("\n## All Memories\n\n");
    index.push_str("| ID | Type | Title | Tags |\n");
    index.push_str("|-----|------|-------|------|\n");
    for mem in memories {
        let id = mem.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        let mem_type = mem
            .get("memory_type")
            .and_then(|v| v.as_str())
            .unwrap_or("note");
        let content = mem
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let title: String = content
            .chars()
            .take(60)
            .collect::<String>()
            .replace('|', "\\|")
            .replace('\n', " ");
        let tags_str = mem
            .get("tags")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let filename = id_to_filename.get(&id).cloned().unwrap_or_default();
        index.push_str(&format!(
            "| {} | {} | [{}]({}/{}.md) | {} |\n",
            id,
            mem_type,
            title,
            pluralize_type(mem_type),
            filename,
            tags_str
        ));
    }

    index
}

/// Parse tags from either comma-separated or JSON array format.
fn parse_tags(tags_str: &str) -> Vec<String> {
    if tags_str.is_empty() {
        return Vec::new();
    }
    if tags_str.starts_with('[') {
        serde_json::from_str(tags_str).unwrap_or_default()
    } else {
        tags_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

/// Sanitize a string for use as a filename.
fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .take(50)
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-').to_lowercase();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

/// Pluralize a memory type for directory naming.
fn pluralize_type(mem_type: &str) -> String {
    match mem_type {
        "summary" => "summaries".to_string(),
        s if s.ends_with('s') => s.to_string(),
        s => format!("{}s", s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── sanitize_filename ──────────────────────────────────────────────

    #[test]
    fn test_sanitize_filename_basic() {
        assert_eq!(sanitize_filename("Hello World!"), "hello-world");
    }

    #[test]
    fn test_sanitize_filename_preserves_alphanumeric() {
        assert_eq!(sanitize_filename("my-note_123"), "my-note_123");
    }

    #[test]
    fn test_sanitize_filename_empty_input() {
        assert_eq!(sanitize_filename(""), "untitled");
    }

    #[test]
    fn test_sanitize_filename_all_special_chars() {
        assert_eq!(sanitize_filename("!!!@@@"), "untitled");
    }

    #[test]
    fn test_sanitize_filename_truncates_long_input() {
        let long_input = "a".repeat(100);
        let result = sanitize_filename(&long_input);
        assert!(result.len() <= 50);
    }

    #[test]
    fn test_sanitize_filename_trims_dashes() {
        assert_eq!(sanitize_filename("  hello  "), "hello");
    }

    // ── pluralize_type ─────────────────────────────────────────────────

    #[test]
    fn test_pluralize_type_note() {
        assert_eq!(pluralize_type("note"), "notes");
    }

    #[test]
    fn test_pluralize_type_todo() {
        assert_eq!(pluralize_type("todo"), "todos");
    }

    #[test]
    fn test_pluralize_type_summary() {
        assert_eq!(pluralize_type("summary"), "summaries");
    }

    #[test]
    fn test_pluralize_type_already_plural() {
        assert_eq!(pluralize_type("issues"), "issues");
    }

    // ── parse_tags ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_tags_empty() {
        assert!(parse_tags("").is_empty());
    }

    #[test]
    fn test_parse_tags_comma_separated() {
        assert_eq!(
            parse_tags("rust, memory, test"),
            vec!["rust", "memory", "test"]
        );
    }

    #[test]
    fn test_parse_tags_json_array() {
        assert_eq!(
            parse_tags(r#"["alpha","beta"]"#),
            vec!["alpha", "beta"]
        );
    }

    // ── format_memory_markdown ─────────────────────────────────────────

    #[test]
    fn test_format_memory_markdown_basic() {
        let mem = json!({
            "id": 1,
            "content": "Hello world",
            "memory_type": "note",
            "tags": "rust,test",
            "importance": 0.8,
            "tier": "permanent",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z"
        });

        let related_map = HashMap::new();
        let id_to_filename = HashMap::new();
        let md = format_memory_markdown(&mem, false, &related_map, &id_to_filename);

        assert!(md.starts_with("---\n"));
        assert!(md.contains("id: 1"));
        assert!(md.contains("type: note"));
        assert!(md.contains("tags: [\"rust\", \"test\"]"));
        assert!(md.contains("importance: 0.80"));
        assert!(md.contains("tier: permanent"));
        assert!(md.contains("Hello world"));
    }

    #[test]
    fn test_format_memory_markdown_with_links() {
        let mem = json!({
            "id": 1,
            "content": "Memory one",
            "memory_type": "note",
            "tags": "",
            "importance": null,
            "tier": "permanent",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": null
        });

        let mut related_map = HashMap::new();
        related_map.insert(1i64, vec![(2i64, "related".to_string())]);

        let mut id_to_filename = HashMap::new();
        id_to_filename.insert(2i64, "2-memory-two".to_string());

        let md = format_memory_markdown(&mem, true, &related_map, &id_to_filename);

        assert!(md.contains("## Related"));
        assert!(md.contains("- related [[2-memory-two]]"));
    }

    #[test]
    fn test_format_memory_markdown_no_links_section_when_empty() {
        let mem = json!({
            "id": 1,
            "content": "Solo memory",
            "memory_type": "note",
            "tags": "",
            "importance": null,
            "tier": "permanent",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": null
        });

        let related_map = HashMap::new();
        let id_to_filename = HashMap::new();
        let md = format_memory_markdown(&mem, true, &related_map, &id_to_filename);

        assert!(!md.contains("## Related"));
    }

    // ── build_index_markdown ───────────────────────────────────────────

    #[test]
    fn test_build_index_markdown_header() {
        let memories = vec![json!({
            "id": 1,
            "content": "Test note",
            "memory_type": "note",
            "tags": "",
        })];
        let mut type_counts = HashMap::new();
        type_counts.insert("note".to_string(), 1);
        let mut id_to_filename = HashMap::new();
        id_to_filename.insert(1i64, "1-test-note".to_string());

        let index = build_index_markdown("mywork", &memories, &type_counts, &id_to_filename);

        assert!(index.contains("# mywork -- Engram Export"));
        assert!(index.contains("**Total memories:** 1"));
        assert!(index.contains("**notes/**"));
        assert!(index.contains("| 1 | note |"));
    }

    // ── memory_export_markdown (requires HandlerContext — skip here) ──
    // Integration tests are in tests/markdown_export.rs
}
