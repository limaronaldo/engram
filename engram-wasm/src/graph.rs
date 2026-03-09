//! Graph traversal algorithms — extracted from engram-core.
//!
//! Provides BFS traversal and shortest-path computation over an edge list.
//! All node identifiers are `u64` (matching engram's `MemoryId = i64` cast).
//!
//! ## Representation
//!
//! Graphs are passed as a flat list of `(from, to)` directed edges.
//! Traversal functions treat graphs as **undirected** by default (edges are
//! followed in both directions), matching engram's `neighborhood` and
//! `find_connected_components` behaviour.
//!
//! ## Invariants
//!
//! - BFS never visits the same node twice.
//! - `max_depth = 0` returns only the start node (no traversal).
//! - Shortest path returns `None` when no path exists.
//! - Self-loops (from == to) are ignored in traversal.

use std::collections::{HashMap, HashSet, VecDeque};

/// A directed edge between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edge {
    pub from: u64,
    pub to: u64,
}

impl Edge {
    pub fn new(from: u64, to: u64) -> Self {
        Self { from, to }
    }
}

/// Build an undirected adjacency list from an edge slice.
fn build_adjacency(edges: &[Edge]) -> HashMap<u64, Vec<u64>> {
    let mut adj: HashMap<u64, Vec<u64>> = HashMap::new();
    for &Edge { from, to } in edges {
        if from == to {
            continue; // Ignore self-loops
        }
        adj.entry(from).or_default().push(to);
        adj.entry(to).or_default().push(from);
    }
    adj
}

/// BFS traversal from `start` up to `max_depth` hops.
///
/// Returns all node IDs reachable from `start` within `max_depth` hops,
/// including `start` itself (at depth 0).
///
/// # Arguments
///
/// * `edges`     — Slice of directed edges. Traversal is undirected.
/// * `start`     — Node to start traversal from.
/// * `max_depth` — Maximum number of hops from start (0 = start only).
///
/// # Returns
///
/// Vec of `(node_id, depth)` pairs, in BFS order.
pub fn bfs(edges: &[Edge], start: u64, max_depth: usize) -> Vec<(u64, usize)> {
    let adj = build_adjacency(edges);

    let mut visited: HashSet<u64> = HashSet::new();
    let mut result: Vec<(u64, usize)> = Vec::new();

    // Queue stores (node, depth)
    let mut queue: VecDeque<(u64, usize)> = VecDeque::new();
    queue.push_back((start, 0));
    visited.insert(start);

    while let Some((node, depth)) = queue.pop_front() {
        result.push((node, depth));

        if depth >= max_depth {
            continue;
        }

        if let Some(neighbors) = adj.get(&node) {
            // Sort neighbors for deterministic output
            let mut sorted_neighbors = neighbors.clone();
            sorted_neighbors.sort_unstable();

            for neighbor in sorted_neighbors {
                if !visited.contains(&neighbor) {
                    visited.insert(neighbor);
                    queue.push_back((neighbor, depth + 1));
                }
            }
        }
    }

    result
}

/// Find the shortest undirected path between `start` and `end`.
///
/// Returns `Some(path)` where `path` is the sequence of node IDs from
/// `start` to `end` inclusive, or `None` if no path exists.
///
/// Uses BFS, which guarantees the shortest path in an unweighted graph.
///
/// # Arguments
///
/// * `edges` — Slice of directed edges. Path-finding is undirected.
/// * `start` — Source node.
/// * `end`   — Target node.
pub fn shortest_path(edges: &[Edge], start: u64, end: u64) -> Option<Vec<u64>> {
    if start == end {
        return Some(vec![start]);
    }

    let adj = build_adjacency(edges);

    let mut visited: HashSet<u64> = HashSet::new();
    // Queue stores (node, path_so_far)
    let mut queue: VecDeque<(u64, Vec<u64>)> = VecDeque::new();

    queue.push_back((start, vec![start]));
    visited.insert(start);

    while let Some((node, path)) = queue.pop_front() {
        if let Some(neighbors) = adj.get(&node) {
            let mut sorted = neighbors.clone();
            sorted.sort_unstable();

            for neighbor in sorted {
                if !visited.contains(&neighbor) {
                    let mut new_path = path.clone();
                    new_path.push(neighbor);

                    if neighbor == end {
                        return Some(new_path);
                    }

                    visited.insert(neighbor);
                    queue.push_back((neighbor, new_path));
                }
            }
        }
    }

    None // No path found
}

/// Find all connected components in the graph.
///
/// Returns a vec of components, each component being a sorted vec of node IDs.
/// Components are returned sorted by size (largest first).
pub fn connected_components(edges: &[Edge], all_nodes: &[u64]) -> Vec<Vec<u64>> {
    let adj = build_adjacency(edges);

    let node_set: HashSet<u64> = all_nodes.iter().copied().collect();
    let mut visited: HashSet<u64> = HashSet::new();
    let mut components: Vec<Vec<u64>> = Vec::new();

    // Ensure isolated nodes (not in any edge) are included
    let mut all_known: HashSet<u64> = node_set.clone();
    for &Edge { from, to } in edges {
        all_known.insert(from);
        all_known.insert(to);
    }

    let mut sorted_nodes: Vec<u64> = all_known.into_iter().collect();
    sorted_nodes.sort_unstable();

    for &node in &sorted_nodes {
        if visited.contains(&node) {
            continue;
        }

        let mut component: Vec<u64> = Vec::new();
        let mut queue: VecDeque<u64> = VecDeque::new();

        queue.push_back(node);
        visited.insert(node);

        while let Some(current) = queue.pop_front() {
            component.push(current);

            if let Some(neighbors) = adj.get(&current) {
                for &neighbor in neighbors {
                    if !visited.contains(&neighbor) {
                        visited.insert(neighbor);
                        queue.push_back(neighbor);
                    }
                }
            }
        }

        component.sort_unstable();
        components.push(component);
    }

    // Sort by size descending
    components.sort_by(|a, b| b.len().cmp(&a.len()));
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(pairs: &[(u64, u64)]) -> Vec<Edge> {
        pairs.iter().map(|&(f, t)| Edge::new(f, t)).collect()
    }

    #[test]
    fn test_bfs_simple_chain() {
        // 1 -> 2 -> 3 -> 4
        let e = edges(&[(1, 2), (2, 3), (3, 4)]);
        let result = bfs(&e, 1, 10);
        let nodes: Vec<u64> = result.iter().map(|(n, _)| *n).collect();
        assert!(nodes.contains(&1));
        assert!(nodes.contains(&2));
        assert!(nodes.contains(&3));
        assert!(nodes.contains(&4));
    }

    #[test]
    fn test_bfs_depth_limit() {
        // 1 -> 2 -> 3 -> 4
        let e = edges(&[(1, 2), (2, 3), (3, 4)]);
        let result = bfs(&e, 1, 1);
        let nodes: Vec<u64> = result.iter().map(|(n, _)| *n).collect();
        assert!(nodes.contains(&1)); // depth 0
        assert!(nodes.contains(&2)); // depth 1
        assert!(!nodes.contains(&3)); // depth 2 — excluded
        assert!(!nodes.contains(&4));
    }

    #[test]
    fn test_bfs_depth_zero() {
        let e = edges(&[(1, 2), (2, 3)]);
        let result = bfs(&e, 1, 0);
        assert_eq!(result, vec![(1, 0)]);
    }

    #[test]
    fn test_bfs_isolated_start() {
        let e = edges(&[(2, 3)]); // Start node 1 has no edges
        let result = bfs(&e, 1, 5);
        assert_eq!(result, vec![(1, 0)]);
    }

    #[test]
    fn test_shortest_path_direct() {
        let e = edges(&[(1, 2)]);
        let path = shortest_path(&e, 1, 2).unwrap();
        assert_eq!(path, vec![1, 2]);
    }

    #[test]
    fn test_shortest_path_multi_hop() {
        // 1 -> 2 -> 3 -> 4
        let e = edges(&[(1, 2), (2, 3), (3, 4)]);
        let path = shortest_path(&e, 1, 4).unwrap();
        assert_eq!(path, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_shortest_path_same_node() {
        let e = edges(&[(1, 2)]);
        let path = shortest_path(&e, 5, 5).unwrap();
        assert_eq!(path, vec![5]);
    }

    #[test]
    fn test_shortest_path_no_path() {
        // Two disconnected components
        let e = edges(&[(1, 2), (3, 4)]);
        let path = shortest_path(&e, 1, 4);
        assert!(path.is_none());
    }

    #[test]
    fn test_shortest_path_prefers_short() {
        // 1 -> 2 -> 3 (length 2) and 1 -> 3 (length 1)
        let e = edges(&[(1, 2), (2, 3), (1, 3)]);
        let path = shortest_path(&e, 1, 3).unwrap();
        assert_eq!(path.len(), 2, "Should prefer the shorter path");
        assert_eq!(path[0], 1);
        assert_eq!(path[path.len() - 1], 3);
    }

    #[test]
    fn test_connected_components() {
        // Two separate components: {1,2,3} and {4,5}
        let e = edges(&[(1, 2), (2, 3), (4, 5)]);
        let comps = connected_components(&e, &[]);
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].len(), 3);
        assert_eq!(comps[1].len(), 2);
    }
}
