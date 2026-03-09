//! Hierarchical memory scoping (T11)
//!
//! Provides a 5-level scope hierarchy:
//! `Global > Org > User > Session > Agent`
//!
//! Scopes are addressed via slash-separated paths, e.g.:
//! - `"global"`
//! - `"global/org:acme"`
//! - `"global/org:acme/user:alice"`
//! - `"global/org:acme/user:alice/session:s123"`
//! - `"global/org:acme/user:alice/session:s123/agent:bot1"`
//!
//! Ancestor inheritance: when searching within a scope a memory is visible if
//! its scope_path is a prefix of (or equal to) the target scope path.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::error::{EngramError, Result};

// ── Level ─────────────────────────────────────────────────────────────────────

/// 5-level hierarchy, ordered from broadest (0) to narrowest (4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ScopeLevel {
    Global = 0,
    Org = 1,
    User = 2,
    Session = 3,
    Agent = 4,
}

impl fmt::Display for ScopeLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScopeLevel::Global => write!(f, "global"),
            ScopeLevel::Org => write!(f, "org"),
            ScopeLevel::User => write!(f, "user"),
            ScopeLevel::Session => write!(f, "session"),
            ScopeLevel::Agent => write!(f, "agent"),
        }
    }
}

// ── Scope ─────────────────────────────────────────────────────────────────────

/// A memory scope with a path-based address.
///
/// The path format mirrors a URI path, e.g.:
/// `"global"`, `"global/org:acme"`, `"global/org:acme/user:alice"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MemoryScope {
    pub level: ScopeLevel,
    pub path: String,
}

impl MemoryScope {
    /// Create a new scope, validating that the path depth matches the level.
    pub fn new(level: ScopeLevel, path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        let expected_segments = level as usize + 1; // Global=1, Org=2, …
        let actual_segments = path.split('/').count();
        if actual_segments != expected_segments {
            return Err(EngramError::InvalidInput(format!(
                "scope path '{}' has {} segment(s) but level {:?} requires {}",
                path, actual_segments, level, expected_segments
            )));
        }
        // The first segment must always be "global".
        if !path.starts_with("global") {
            return Err(EngramError::InvalidInput(format!(
                "scope path must start with 'global', got '{}'",
                path
            )));
        }
        Ok(Self { level, path })
    }

    /// Shorthand constructor for the global scope.
    pub fn global() -> Self {
        Self {
            level: ScopeLevel::Global,
            path: "global".to_string(),
        }
    }

    /// Parse a path string into a `MemoryScope`.
    ///
    /// The level is inferred from the number of `/`-separated segments:
    /// 1 → Global, 2 → Org, 3 → User, 4 → Session, 5 → Agent.
    pub fn parse(path: &str) -> Result<Self> {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.is_empty() || segments[0] != "global" {
            return Err(EngramError::InvalidInput(format!(
                "scope path must start with 'global', got '{}'",
                path
            )));
        }
        let level = match segments.len() {
            1 => ScopeLevel::Global,
            2 => ScopeLevel::Org,
            3 => ScopeLevel::User,
            4 => ScopeLevel::Session,
            5 => ScopeLevel::Agent,
            n => {
                return Err(EngramError::InvalidInput(format!(
                    "scope path has {} segments; maximum supported depth is 5 (Agent)",
                    n
                )))
            }
        };
        Ok(Self {
            level,
            path: path.to_string(),
        })
    }

    /// Return the immediate parent scope, or `None` for the Global scope.
    pub fn parent(&self) -> Option<MemoryScope> {
        if self.level == ScopeLevel::Global {
            return None;
        }
        // Strip the last path segment.
        let last_slash = self.path.rfind('/')?;
        let parent_path = &self.path[..last_slash];
        // Level is one step coarser — safe because we checked level != Global.
        let parent_level = match self.level {
            ScopeLevel::Org => ScopeLevel::Global,
            ScopeLevel::User => ScopeLevel::Org,
            ScopeLevel::Session => ScopeLevel::User,
            ScopeLevel::Agent => ScopeLevel::Session,
            ScopeLevel::Global => unreachable!(),
        };
        Some(MemoryScope {
            level: parent_level,
            path: parent_path.to_string(),
        })
    }

    /// Return all ancestor scopes from the immediate parent up to (and
    /// including) the Global scope.  Order: closest ancestor first.
    pub fn ancestors(&self) -> Vec<MemoryScope> {
        let mut result = Vec::new();
        let mut current = self.parent();
        while let Some(scope) = current {
            current = scope.parent();
            result.push(scope);
        }
        result
    }

    /// Returns `true` if `self` is an ancestor of (or equal to) `other`.
    ///
    /// Equivalently: `self` "contains" `other` if `other.path` starts with
    /// `self.path` followed by `/` (or is identical).
    pub fn contains(&self, other: &MemoryScope) -> bool {
        if self == other {
            return true;
        }
        // `other` must be strictly deeper.
        if other.level <= self.level {
            return false;
        }
        // Path prefix check: other.path must start with self.path + "/".
        other.path.starts_with(&format!("{}/", self.path))
    }
}

impl fmt::Display for MemoryScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path)
    }
}

// ── Tree ──────────────────────────────────────────────────────────────────────

/// A node in the scope tree returned by [`scope_tree`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeNode {
    pub scope: MemoryScope,
    pub memory_count: i64,
    pub children: Vec<ScopeNode>,
}

// ── Storage functions ─────────────────────────────────────────────────────────

/// Set the scope of a memory by updating its `scope_path` column.
pub fn set_scope(conn: &Connection, memory_id: i64, scope: &MemoryScope) -> Result<()> {
    let rows = conn.execute(
        "UPDATE memories SET scope_path = ?1 WHERE id = ?2",
        params![scope.path, memory_id],
    )?;
    if rows == 0 {
        return Err(EngramError::NotFound(memory_id));
    }
    Ok(())
}

/// Read the current scope of a memory.
pub fn get_scope(conn: &Connection, memory_id: i64) -> Result<MemoryScope> {
    let path: Option<String> = conn
        .query_row(
            "SELECT scope_path FROM memories WHERE id = ?1",
            params![memory_id],
            |row| row.get(0),
        )
        .optional()?;

    match path {
        Some(p) => MemoryScope::parse(&p),
        None => Err(EngramError::NotFound(memory_id)),
    }
}

/// Return every distinct scope that at least one memory belongs to.
pub fn list_scopes(conn: &Connection) -> Result<Vec<MemoryScope>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT scope_path FROM memories WHERE scope_path IS NOT NULL")?;
    let scopes = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .filter_map(|path| MemoryScope::parse(&path).ok())
        .collect();
    Ok(scopes)
}

/// Move a memory to a different scope.
pub fn move_scope(conn: &Connection, memory_id: i64, new_scope: &MemoryScope) -> Result<()> {
    set_scope(conn, memory_id, new_scope)
}

/// Search for memories whose content matches `query` within `scope` **or any
/// ancestor scope** (child sees parent memories).
///
/// The match is a simple case-insensitive substring search.  Returns memory
/// IDs ordered by id descending.
///
/// Ancestor inheritance is implemented via `scope_path LIKE '<prefix>%'` which
/// is equivalent to "scope_path starts with the ancestor path".  Because we
/// want the scope itself *plus* all its ancestors, we collect the set of
/// ancestor paths (including the scope itself) and build an OR clause.
pub fn search_scoped(conn: &Connection, query: &str, scope: &MemoryScope) -> Result<Vec<i64>> {
    // Build list: scope itself + all ancestors.
    let mut paths: Vec<String> = vec![scope.path.clone()];
    for ancestor in scope.ancestors() {
        paths.push(ancestor.path);
    }

    // Construct: (scope_path = 'global' OR scope_path LIKE 'global/org:acme%' …)
    // We use the LIKE approach so that sub-scopes of each ancestor are also included.
    // Actually: we want to find memories that are AT those exact scopes, not at
    // any deeper scope — a memory at "global/org:acme/user:bob" should not be
    // visible when searching from "global/org:acme/user:alice" even though
    // "global/org:acme" is an ancestor of both.
    //
    // Correct semantic: memory is visible if its scope_path is one of:
    //   - the search scope itself, OR
    //   - any ancestor scope of the search scope.
    // This is an exact-match OR, not a prefix match.
    let placeholders: Vec<String> = paths.iter().map(|_| "?".to_string()).collect();
    let in_clause = placeholders.join(", ");
    let sql = format!(
        "SELECT id FROM memories WHERE content LIKE ? AND scope_path IN ({}) ORDER BY id DESC",
        in_clause
    );

    let like_query = format!("%{}%", query);
    let mut stmt = conn.prepare(&sql)?;

    // Build params: first the LIKE value, then each path.
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    param_values.push(Box::new(like_query));
    for p in &paths {
        param_values.push(Box::new(p.clone()));
    }

    let refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|b| b.as_ref()).collect();

    let ids: Vec<i64> = stmt
        .query_map(refs.as_slice(), |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(ids)
}

/// Build a scope tree from all distinct scope paths stored in the database.
///
/// Each node carries the number of memories whose `scope_path` exactly matches
/// that scope (i.e., not counting descendant memories).
pub fn scope_tree(conn: &Connection) -> Result<Vec<ScopeNode>> {
    // Fetch all (scope_path, count) pairs.
    let mut stmt = conn.prepare(
        "SELECT scope_path, COUNT(*) as cnt FROM memories
         WHERE scope_path IS NOT NULL
         GROUP BY scope_path
         ORDER BY scope_path",
    )?;

    let rows: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    // Convert to ScopeNode list (flat).
    let mut nodes: Vec<ScopeNode> = rows
        .into_iter()
        .filter_map(|(path, count)| {
            MemoryScope::parse(&path).ok().map(|scope| ScopeNode {
                scope,
                memory_count: count,
                children: Vec::new(),
            })
        })
        .collect();

    // Sort by path depth ascending so parents are processed before children.
    nodes.sort_by_key(|n| n.scope.level as usize);

    build_tree(nodes)
}

/// Recursively nest nodes into a proper tree.
fn build_tree(mut nodes: Vec<ScopeNode>) -> Result<Vec<ScopeNode>> {
    // Process from deepest to shallowest so we can attach children.
    nodes.sort_by(|a, b| (b.scope.level as usize).cmp(&(a.scope.level as usize)));

    // We'll accumulate root nodes here.
    let mut roots: Vec<ScopeNode> = Vec::new();

    // For each node (deepest first), find its parent in the remaining set.
    // Simple O(n²) approach — scope trees are small in practice.
    while let Some(node) = nodes.pop() {
        if node.scope.level == ScopeLevel::Global {
            roots.push(node);
            continue;
        }
        // Find the parent among remaining nodes or already-placed roots.
        let parent_path = match node.scope.parent() {
            Some(p) => p.path,
            None => {
                roots.push(node);
                continue;
            }
        };
        // Try to attach to an existing node in `nodes` (not yet placed).
        if let Some(parent) = nodes.iter_mut().find(|n| n.scope.path == parent_path) {
            parent.children.push(node);
        } else {
            // Parent not found — treat as orphan root (defensive).
            roots.push(node);
        }
    }

    Ok(roots)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY,
                content TEXT NOT NULL,
                scope_path TEXT DEFAULT 'global'
            );",
        )
        .unwrap();
        conn
    }

    fn insert(conn: &Connection, id: i64, content: &str, scope: &str) {
        conn.execute(
            "INSERT INTO memories (id, content, scope_path) VALUES (?1, ?2, ?3)",
            params![id, content, scope],
        )
        .unwrap();
    }

    // ── 1. Parse scope from path string ───────────────────────────────────────

    #[test]
    fn test_parse_global() {
        let s = MemoryScope::parse("global").unwrap();
        assert_eq!(s.level, ScopeLevel::Global);
        assert_eq!(s.path, "global");
    }

    #[test]
    fn test_parse_org() {
        let s = MemoryScope::parse("global/org:acme").unwrap();
        assert_eq!(s.level, ScopeLevel::Org);
        assert_eq!(s.path, "global/org:acme");
    }

    #[test]
    fn test_parse_user() {
        let s = MemoryScope::parse("global/org:acme/user:alice").unwrap();
        assert_eq!(s.level, ScopeLevel::User);
    }

    #[test]
    fn test_parse_invalid_prefix() {
        assert!(MemoryScope::parse("org:acme").is_err());
    }

    #[test]
    fn test_parse_too_deep() {
        assert!(MemoryScope::parse("global/org:a/user:b/session:c/agent:d/extra:e").is_err());
    }

    // ── 2. Parent / ancestors traversal ───────────────────────────────────────

    #[test]
    fn test_parent() {
        let agent = MemoryScope::parse("global/org:acme/user:alice/session:s1/agent:bot").unwrap();
        let session = agent.parent().unwrap();
        assert_eq!(session.level, ScopeLevel::Session);
        assert_eq!(session.path, "global/org:acme/user:alice/session:s1");

        let user = session.parent().unwrap();
        assert_eq!(user.level, ScopeLevel::User);

        let org = user.parent().unwrap();
        assert_eq!(org.level, ScopeLevel::Org);

        let global = org.parent().unwrap();
        assert_eq!(global.level, ScopeLevel::Global);
        assert!(global.parent().is_none());
    }

    #[test]
    fn test_ancestors() {
        let user = MemoryScope::parse("global/org:acme/user:alice").unwrap();
        let ancestors = user.ancestors();
        assert_eq!(ancestors.len(), 2);
        assert_eq!(ancestors[0].level, ScopeLevel::Org);
        assert_eq!(ancestors[1].level, ScopeLevel::Global);
    }

    // ── 3. Contains check ─────────────────────────────────────────────────────

    #[test]
    fn test_contains_parent_contains_child() {
        let global = MemoryScope::global();
        let org = MemoryScope::parse("global/org:acme").unwrap();
        let user = MemoryScope::parse("global/org:acme/user:alice").unwrap();

        assert!(global.contains(&org));
        assert!(global.contains(&user));
        assert!(org.contains(&user));
    }

    #[test]
    fn test_contains_child_does_not_contain_parent() {
        let global = MemoryScope::global();
        let org = MemoryScope::parse("global/org:acme").unwrap();
        assert!(!org.contains(&global));
    }

    #[test]
    fn test_contains_sibling_false() {
        let alice = MemoryScope::parse("global/org:acme/user:alice").unwrap();
        let bob = MemoryScope::parse("global/org:acme/user:bob").unwrap();
        assert!(!alice.contains(&bob));
        assert!(!bob.contains(&alice));
    }

    #[test]
    fn test_contains_self_true() {
        let s = MemoryScope::global();
        assert!(s.contains(&s));
    }

    // ── 4. Set and get scope ──────────────────────────────────────────────────

    #[test]
    fn test_set_and_get_scope() {
        let conn = setup_db();
        insert(&conn, 1, "hello", "global");

        let new_scope = MemoryScope::parse("global/org:acme").unwrap();
        set_scope(&conn, 1, &new_scope).unwrap();

        let retrieved = get_scope(&conn, 1).unwrap();
        assert_eq!(retrieved, new_scope);
    }

    #[test]
    fn test_get_scope_not_found() {
        let conn = setup_db();
        let err = get_scope(&conn, 999).unwrap_err();
        assert!(matches!(err, EngramError::NotFound(999)));
    }

    #[test]
    fn test_set_scope_not_found() {
        let conn = setup_db();
        let scope = MemoryScope::global();
        let err = set_scope(&conn, 999, &scope).unwrap_err();
        assert!(matches!(err, EngramError::NotFound(999)));
    }

    // ── 5. Search scoped with ancestor inheritance ────────────────────────────

    #[test]
    fn test_search_scoped_ancestor_inheritance() {
        let conn = setup_db();
        // global memory — visible from anywhere
        insert(&conn, 1, "common knowledge", "global");
        // org-level memory — visible from org and below
        insert(&conn, 2, "acme org policy", "global/org:acme");
        // user-level memory — visible only from that user scope
        insert(
            &conn,
            3,
            "alice personal note",
            "global/org:acme/user:alice",
        );
        // different user — NOT visible to alice
        insert(&conn, 4, "bob personal note", "global/org:acme/user:bob");

        let alice_scope = MemoryScope::parse("global/org:acme/user:alice").unwrap();

        // "knowledge" only in global memory
        let ids = search_scoped(&conn, "knowledge", &alice_scope).unwrap();
        assert!(ids.contains(&1), "global memory should be visible");
        assert!(!ids.contains(&3));

        // "policy" only in org memory
        let ids = search_scoped(&conn, "policy", &alice_scope).unwrap();
        assert!(ids.contains(&2), "org memory should be visible");

        // "alice" in alice's own memory
        let ids = search_scoped(&conn, "alice", &alice_scope).unwrap();
        assert!(ids.contains(&3));

        // "bob" — bob's memory should NOT appear when searching from alice scope
        let ids = search_scoped(&conn, "bob", &alice_scope).unwrap();
        assert!(
            !ids.contains(&4),
            "bob's memory must not be visible to alice"
        );
    }

    // ── 6. Move scope ─────────────────────────────────────────────────────────

    #[test]
    fn test_move_scope() {
        let conn = setup_db();
        insert(&conn, 1, "memory", "global");

        let new_scope = MemoryScope::parse("global/org:acme/user:alice").unwrap();
        move_scope(&conn, 1, &new_scope).unwrap();

        let retrieved = get_scope(&conn, 1).unwrap();
        assert_eq!(retrieved.path, "global/org:acme/user:alice");
    }

    // ── 7. Scope tree construction ────────────────────────────────────────────

    #[test]
    fn test_scope_tree() {
        let conn = setup_db();
        insert(&conn, 1, "a", "global");
        insert(&conn, 2, "b", "global");
        insert(&conn, 3, "c", "global/org:acme");
        insert(&conn, 4, "d", "global/org:acme/user:alice");

        let tree = scope_tree(&conn).unwrap();
        // There should be at least one root (global).
        let global_node = tree.iter().find(|n| n.scope.level == ScopeLevel::Global);
        assert!(global_node.is_some(), "global node must be present");

        let global_node = global_node.unwrap();
        assert_eq!(global_node.memory_count, 2); // id 1 and 2
    }

    // ── 8. Global scope has no parent ─────────────────────────────────────────

    #[test]
    fn test_global_has_no_parent() {
        let global = MemoryScope::global();
        assert!(global.parent().is_none());
        assert!(global.ancestors().is_empty());
    }

    // ── Display ───────────────────────────────────────────────────────────────

    #[test]
    fn test_display_scope_level() {
        assert_eq!(ScopeLevel::Global.to_string(), "global");
        assert_eq!(ScopeLevel::Org.to_string(), "org");
        assert_eq!(ScopeLevel::User.to_string(), "user");
        assert_eq!(ScopeLevel::Session.to_string(), "session");
        assert_eq!(ScopeLevel::Agent.to_string(), "agent");
    }

    #[test]
    fn test_display_memory_scope() {
        let s = MemoryScope::parse("global/org:acme/user:alice").unwrap();
        assert_eq!(s.to_string(), "global/org:acme/user:alice");
    }

    // ── List scopes ───────────────────────────────────────────────────────────

    #[test]
    fn test_list_scopes() {
        let conn = setup_db();
        insert(&conn, 1, "a", "global");
        insert(&conn, 2, "b", "global/org:acme");
        insert(&conn, 3, "c", "global/org:acme");

        let scopes = list_scopes(&conn).unwrap();
        assert_eq!(scopes.len(), 2);
        let paths: Vec<&str> = scopes.iter().map(|s| s.path.as_str()).collect();
        assert!(paths.contains(&"global"));
        assert!(paths.contains(&"global/org:acme"));
    }
}
