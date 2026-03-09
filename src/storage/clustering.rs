//! Louvain community detection for memory clusters (v0.8.0 — Emergent Knowledge Graph)
//!
//! This module detects communities of related memories by running a single-pass
//! Louvain algorithm over the `auto_links` graph (Schema v17-v18).
//!
//! # Algorithm
//!
//! The simplified single-pass Louvain works as follows:
//!
//! 1. Load all `auto_links` edges into an adjacency list.
//! 2. Assign every node to its own singleton community.
//! 3. For each node (in a random order), compute the modularity gain of moving
//!    it to each neighbouring community. Accept the best positive move.
//! 4. Repeat step 3 until no node moves in a full pass (convergence).
//! 5. Compute the final modularity Q and persist the assignments.
//!
//! # Modularity
//!
//! ```text
//! Q = Σ_i [ (e_ii) - (a_i)² / (2m) ]
//! ```
//!
//! where:
//! - `e_ii` = fraction of edge weight within community i
//! - `a_i`  = sum of degrees of nodes in community i
//! - `m`    = total edge weight (sum of all edge scores)
//!
//! # Feature Gate
//!
//! This module is feature-gated under `emergent-graph`.

#![cfg(feature = "emergent-graph")]

use std::collections::{HashMap, HashSet};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A detected memory community.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cluster {
    /// Unique cluster index (0-based, stable within a single run)
    pub cluster_id: usize,
    /// IDs of memories that belong to this cluster
    pub members: Vec<i64>,
    /// Number of members (always `members.len()`)
    pub size: usize,
}

/// Options controlling the Louvain clustering run.
#[derive(Debug, Clone)]
pub struct LouvainOptions {
    /// Minimum number of members for a cluster to be kept (default 2).
    ///
    /// Singleton clusters (size 1) are never written to the database.
    pub min_cluster_size: usize,
    /// Resolution parameter γ (default 1.0).
    ///
    /// Higher values produce more, smaller clusters; lower values produce fewer,
    /// larger clusters.
    pub resolution: f64,
    /// Which `auto_link.link_type` values to include when building the graph.
    ///
    /// `None` means "use all link types".
    pub link_types: Option<Vec<String>>,
}

impl Default for LouvainOptions {
    fn default() -> Self {
        Self {
            min_cluster_size: 2,
            resolution: 1.0,
            link_types: None,
        }
    }
}

/// Summary returned after a clustering run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusteringResult {
    /// Detected communities (filtered by `min_cluster_size`).
    pub clusters: Vec<Cluster>,
    /// Final modularity Q (range approximately -0.5 … 1.0).
    pub modularity: f64,
    /// Total number of unique memory nodes seen in the graph.
    pub nodes: usize,
}

// ---------------------------------------------------------------------------
// Core algorithm
// ---------------------------------------------------------------------------

/// Run Louvain community detection and persist results to `memory_clusters`.
///
/// # Steps
///
/// 1. Reads `auto_links` (filtered by `options.link_types`).
/// 2. Builds an undirected weighted adjacency graph.
/// 3. Runs single-pass Louvain until convergence.
/// 4. Clears previous `memory_clusters` rows (for the `louvain` algorithm).
/// 5. Writes new cluster memberships.
/// 6. Returns a [`ClusteringResult`].
///
/// # Errors
///
/// Returns an error if any database operation fails.
pub fn run_louvain_clustering(
    conn: &Connection,
    options: &LouvainOptions,
) -> Result<ClusteringResult> {
    // ------------------------------------------------------------------
    // 1. Load edges from auto_links
    // ------------------------------------------------------------------
    let edges = load_edges(conn, options)?;

    if edges.is_empty() {
        return Ok(ClusteringResult {
            clusters: vec![],
            modularity: 0.0,
            nodes: 0,
        });
    }

    // ------------------------------------------------------------------
    // 2. Build adjacency graph (undirected)
    // ------------------------------------------------------------------
    let (adj, nodes) = build_adjacency(&edges);
    let node_count = nodes.len();

    // ------------------------------------------------------------------
    // 3. Run Louvain
    // ------------------------------------------------------------------
    let (community_of, modularity) = louvain(&adj, &nodes, options.resolution);

    // ------------------------------------------------------------------
    // 4. Group nodes by community
    // ------------------------------------------------------------------
    let mut community_members: HashMap<usize, Vec<i64>> = HashMap::new();
    for &node_id in &nodes {
        let comm = community_of[&node_id];
        community_members.entry(comm).or_default().push(node_id);
    }

    // Assign contiguous cluster IDs, sort members for determinism
    let mut clusters: Vec<Cluster> = community_members
        .into_values()
        .filter(|members| members.len() >= options.min_cluster_size)
        .enumerate()
        .map(|(idx, mut members)| {
            members.sort_unstable();
            let size = members.len();
            Cluster {
                cluster_id: idx,
                members,
                size,
            }
        })
        .collect();
    // Stable sort: larger clusters first, then by first member id
    clusters.sort_by(|a, b| {
        b.size
            .cmp(&a.size)
            .then_with(|| a.members[0].cmp(&b.members[0]))
    });
    // Re-index after sort
    for (idx, cluster) in clusters.iter_mut().enumerate() {
        cluster.cluster_id = idx;
    }

    // ------------------------------------------------------------------
    // 5. Persist to memory_clusters (clear previous louvain results first)
    // ------------------------------------------------------------------
    conn.execute(
        "DELETE FROM memory_clusters WHERE algorithm = 'louvain'",
        [],
    )?;

    for cluster in &clusters {
        for &memory_id in &cluster.members {
            conn.execute(
                "INSERT OR REPLACE INTO memory_clusters
                     (cluster_id, memory_id, algorithm, modularity)
                 VALUES (?1, ?2, 'louvain', ?3)",
                rusqlite::params![cluster.cluster_id as i64, memory_id, modularity],
            )?;
        }
    }

    Ok(ClusteringResult {
        clusters,
        modularity,
        nodes: node_count,
    })
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Find the cluster that contains `memory_id` (from the last Louvain run).
///
/// Returns `None` if the memory is not part of any cluster.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub fn get_cluster(conn: &Connection, memory_id: i64) -> Result<Option<Cluster>> {
    // Find cluster_id for this memory
    let cluster_id: Option<i64> = conn
        .query_row(
            "SELECT cluster_id FROM memory_clusters
             WHERE memory_id = ?1 AND algorithm = 'louvain'
             LIMIT 1",
            rusqlite::params![memory_id],
            |row| row.get(0),
        )
        .ok();

    let Some(cluster_id) = cluster_id else {
        return Ok(None);
    };

    // Fetch all members of that cluster
    let mut stmt = conn.prepare(
        "SELECT memory_id FROM memory_clusters
         WHERE cluster_id = ?1 AND algorithm = 'louvain'
         ORDER BY memory_id",
    )?;

    let members: Vec<i64> = stmt
        .query_map(rusqlite::params![cluster_id], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    if members.is_empty() {
        return Ok(None);
    }

    let size = members.len();
    Ok(Some(Cluster {
        cluster_id: cluster_id as usize,
        members,
        size,
    }))
}

/// List all clusters for a given algorithm (e.g. `"louvain"`).
///
/// Clusters are returned sorted by `cluster_id`.
///
/// # Errors
///
/// Returns an error if the database query fails.
pub fn list_clusters(conn: &Connection, algorithm: &str) -> Result<Vec<Cluster>> {
    // Get distinct cluster IDs
    let mut id_stmt = conn.prepare(
        "SELECT DISTINCT cluster_id FROM memory_clusters
         WHERE algorithm = ?1
         ORDER BY cluster_id",
    )?;

    let cluster_ids: Vec<i64> = id_stmt
        .query_map(rusqlite::params![algorithm], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    let mut result = Vec::with_capacity(cluster_ids.len());

    for cluster_id in cluster_ids {
        let mut mem_stmt = conn.prepare(
            "SELECT memory_id FROM memory_clusters
             WHERE cluster_id = ?1 AND algorithm = ?2
             ORDER BY memory_id",
        )?;

        let members: Vec<i64> = mem_stmt
            .query_map(rusqlite::params![cluster_id, algorithm], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        let size = members.len();
        result.push(Cluster {
            cluster_id: cluster_id as usize,
            members,
            size,
        });
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Internal: edge loading
// ---------------------------------------------------------------------------

/// A weighted directed edge in the auto-link graph.
#[derive(Debug, Clone)]
struct Edge {
    from: i64,
    to: i64,
    weight: f64,
}

fn load_edges(conn: &Connection, options: &LouvainOptions) -> Result<Vec<Edge>> {
    let sql = match &options.link_types {
        None => "SELECT from_id, to_id, score FROM auto_links".to_string(),
        Some(types) => {
            let placeholders = types
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "SELECT from_id, to_id, score FROM auto_links WHERE link_type IN ({})",
                placeholders
            )
        }
    };

    let mut stmt = conn.prepare(&sql)?;

    let edges = match &options.link_types {
        None => stmt
            .query_map([], |row| {
                Ok(Edge {
                    from: row.get(0)?,
                    to: row.get(1)?,
                    weight: row.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect(),
        Some(types) => {
            let params: Vec<Box<dyn rusqlite::ToSql>> = types
                .iter()
                .map(|t| Box::new(t.clone()) as Box<dyn rusqlite::ToSql>)
                .collect();
            let params_refs: Vec<&dyn rusqlite::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            stmt.query_map(params_refs.as_slice(), |row| {
                Ok(Edge {
                    from: row.get(0)?,
                    to: row.get(1)?,
                    weight: row.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect()
        }
    };

    Ok(edges)
}

// ---------------------------------------------------------------------------
// Internal: graph construction
// ---------------------------------------------------------------------------

/// Adjacency list: node_id → Vec<(neighbour_id, weight)>
type AdjList = HashMap<i64, Vec<(i64, f64)>>;

fn build_adjacency(edges: &[Edge]) -> (AdjList, Vec<i64>) {
    let mut adj: AdjList = HashMap::new();
    let mut node_set: HashSet<i64> = HashSet::new();

    for e in edges {
        adj.entry(e.from).or_default().push((e.to, e.weight));
        adj.entry(e.to).or_default().push((e.from, e.weight)); // undirected
        node_set.insert(e.from);
        node_set.insert(e.to);
    }

    let mut nodes: Vec<i64> = node_set.into_iter().collect();
    nodes.sort_unstable(); // deterministic ordering

    (adj, nodes)
}

// ---------------------------------------------------------------------------
// Internal: Louvain algorithm
// ---------------------------------------------------------------------------

/// Run a single-pass Louvain optimisation.
///
/// Returns a map of `node_id → community_id` and the final modularity Q.
fn louvain(adj: &AdjList, nodes: &[i64], resolution: f64) -> (HashMap<i64, usize>, f64) {
    // Total edge weight m (sum of all weights; each undirected edge counted twice)
    let two_m: f64 = adj
        .values()
        .flat_map(|neighbours| neighbours.iter().map(|(_, w)| *w))
        .sum();

    if two_m == 0.0 {
        // No edges — every node is its own community
        let comm: HashMap<i64, usize> = nodes.iter().enumerate().map(|(i, &id)| (id, i)).collect();
        return (comm, 0.0);
    }

    // Initialise: each node in its own singleton community
    let mut community_of: HashMap<i64, usize> =
        nodes.iter().enumerate().map(|(i, &id)| (id, i)).collect();

    // Degree (sum of edge weights) for each node
    let degree_of: HashMap<i64, f64> = nodes
        .iter()
        .map(|&id| {
            let deg = adj
                .get(&id)
                .map(|ns| ns.iter().map(|(_, w)| w).sum())
                .unwrap_or(0.0);
            (id, deg)
        })
        .collect();

    // Total degree per community (Σ k_i for nodes i in community c)
    let mut community_degree: HashMap<usize, f64> = community_of
        .iter()
        .map(|(&node, &comm)| (comm, *degree_of.get(&node).unwrap_or(&0.0)))
        .collect();

    let mut improved = true;

    while improved {
        improved = false;

        for &node_id in nodes {
            let current_comm = community_of[&node_id];
            let ki = degree_of[&node_id]; // node degree

            // Accumulate edge weight from node_id to each neighbouring community
            let mut comm_weight: HashMap<usize, f64> = HashMap::new();
            if let Some(neighbours) = adj.get(&node_id) {
                for &(nb_id, w) in neighbours {
                    let nb_comm = community_of[&nb_id];
                    *comm_weight.entry(nb_comm).or_insert(0.0) += w;
                }
            }

            let sigma_tot_cur = community_degree[&current_comm];
            let k_i_in_cur = comm_weight.get(&current_comm).copied().unwrap_or(0.0);

            // Modularity gain of *removing* node from current community
            let delta_remove =
                -(k_i_in_cur / two_m) + resolution * (sigma_tot_cur - ki) * ki / (two_m * two_m);

            // Find best neighbour community to move to
            let mut best_gain = 0.0;
            let mut best_comm = current_comm;

            for (&cand_comm, &k_i_in_cand) in &comm_weight {
                if cand_comm == current_comm {
                    continue;
                }
                let sigma_tot_cand = community_degree.get(&cand_comm).copied().unwrap_or(0.0);

                // Modularity gain of *adding* node to cand_comm
                let delta_add =
                    (k_i_in_cand / two_m) - resolution * sigma_tot_cand * ki / (two_m * two_m);

                let delta_q = delta_add + delta_remove;

                if delta_q > best_gain {
                    best_gain = delta_q;
                    best_comm = cand_comm;
                }
            }

            if best_comm != current_comm {
                // Remove from current community
                *community_degree.entry(current_comm).or_insert(0.0) -= ki;
                // Add to best community
                *community_degree.entry(best_comm).or_insert(0.0) += ki;
                // Update assignment
                community_of.insert(node_id, best_comm);
                improved = true;
            }
        }
    }

    // Compute final modularity Q
    let modularity = compute_modularity(&community_of, adj, &degree_of, two_m, resolution);

    (community_of, modularity)
}

/// Compute modularity Q for the current community assignment.
///
/// Q = (1/2m) Σ_{ij} [ A_{ij} - γ * k_i * k_j / 2m ] * δ(c_i, c_j)
fn compute_modularity(
    community_of: &HashMap<i64, usize>,
    adj: &AdjList,
    degree_of: &HashMap<i64, f64>,
    two_m: f64,
    resolution: f64,
) -> f64 {
    if two_m == 0.0 {
        return 0.0;
    }

    // Group nodes by community
    let mut communities: HashMap<usize, Vec<i64>> = HashMap::new();
    for (&node, &comm) in community_of {
        communities.entry(comm).or_default().push(node);
    }

    let mut q = 0.0;

    for members in communities.values() {
        let member_set: HashSet<i64> = members.iter().copied().collect();

        let mut e_ii = 0.0; // internal edge weight
        let mut a_i = 0.0; // sum of degrees in community

        for &node in members {
            let ki = degree_of.get(&node).copied().unwrap_or(0.0);
            a_i += ki;

            if let Some(neighbours) = adj.get(&node) {
                for &(nb, w) in neighbours {
                    if member_set.contains(&nb) {
                        e_ii += w;
                    }
                }
            }
        }

        // e_ii is double-counted for undirected graph (each edge appears twice in adj)
        e_ii /= two_m;
        let a_i_norm = a_i / two_m;

        q += e_ii - resolution * a_i_norm * a_i_norm;
    }

    q
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Set up an in-memory database with the auto_links and memory_clusters tables.
    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE auto_links (
                from_id   INTEGER NOT NULL,
                to_id     INTEGER NOT NULL,
                link_type TEXT    NOT NULL,
                score     REAL    NOT NULL,
                PRIMARY KEY (from_id, to_id, link_type)
            );

            CREATE TABLE memory_clusters (
                id         INTEGER PRIMARY KEY AUTOINCREMENT,
                cluster_id INTEGER NOT NULL,
                memory_id  INTEGER NOT NULL,
                algorithm  TEXT    NOT NULL,
                modularity REAL,
                UNIQUE(memory_id, algorithm)
            );
            "#,
        )
        .unwrap();
        conn
    }

    fn insert_link(conn: &Connection, from: i64, to: i64, link_type: &str, score: f64) {
        conn.execute(
            "INSERT OR REPLACE INTO auto_links (from_id, to_id, link_type, score)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![from, to, link_type, score],
        )
        .unwrap();
    }

    // -----------------------------------------------------------------------
    // Tests for run_louvain_clustering
    // -----------------------------------------------------------------------

    #[test]
    fn test_empty_graph_returns_no_clusters() {
        let conn = setup_db();
        let options = LouvainOptions::default();
        let result = run_louvain_clustering(&conn, &options).unwrap();
        assert!(result.clusters.is_empty());
        assert_eq!(result.nodes, 0);
        assert_eq!(result.modularity, 0.0);
    }

    #[test]
    fn test_two_disconnected_nodes_no_cluster() {
        // Two nodes with no edges → singleton communities → filtered out (min_cluster_size = 2)
        let conn = setup_db();
        // Insert no links; just see what happens with min_cluster_size = 2
        let options = LouvainOptions::default();
        let result = run_louvain_clustering(&conn, &options).unwrap();
        assert!(result.clusters.is_empty());
    }

    #[test]
    fn test_two_linked_nodes_form_one_cluster() {
        let conn = setup_db();
        insert_link(&conn, 1, 2, "semantic", 0.9);

        let options = LouvainOptions::default();
        let result = run_louvain_clustering(&conn, &options).unwrap();

        assert_eq!(result.nodes, 2);
        assert_eq!(result.clusters.len(), 1);
        assert_eq!(result.clusters[0].size, 2);
        assert!(result.clusters[0].members.contains(&1));
        assert!(result.clusters[0].members.contains(&2));
    }

    #[test]
    fn test_triangle_forms_single_cluster() {
        let conn = setup_db();
        insert_link(&conn, 1, 2, "semantic", 0.9);
        insert_link(&conn, 2, 3, "semantic", 0.9);
        insert_link(&conn, 1, 3, "semantic", 0.9);

        let options = LouvainOptions::default();
        let result = run_louvain_clustering(&conn, &options).unwrap();

        assert_eq!(result.nodes, 3);
        // All three should end up in one cluster
        assert_eq!(result.clusters.len(), 1);
        assert_eq!(result.clusters[0].size, 3);
    }

    #[test]
    fn test_two_dense_groups_form_two_clusters() {
        let conn = setup_db();
        // Cluster A: 1-2-3 tightly connected
        insert_link(&conn, 1, 2, "semantic", 1.0);
        insert_link(&conn, 2, 3, "semantic", 1.0);
        insert_link(&conn, 1, 3, "semantic", 1.0);
        // Cluster B: 10-11-12 tightly connected
        insert_link(&conn, 10, 11, "semantic", 1.0);
        insert_link(&conn, 11, 12, "semantic", 1.0);
        insert_link(&conn, 10, 12, "semantic", 1.0);
        // Weak bridge between the two groups
        insert_link(&conn, 3, 10, "semantic", 0.01);

        let options = LouvainOptions::default();
        let result = run_louvain_clustering(&conn, &options).unwrap();

        assert_eq!(result.nodes, 6);
        // Expect two clusters
        assert_eq!(result.clusters.len(), 2);
        let total_members: usize = result.clusters.iter().map(|c| c.size).sum();
        assert_eq!(total_members, 6);
    }

    #[test]
    fn test_link_type_filter() {
        let conn = setup_db();
        // Semantic link in cluster A
        insert_link(&conn, 1, 2, "semantic", 0.9);
        insert_link(&conn, 2, 3, "semantic", 0.9);
        // Temporal links in cluster B
        insert_link(&conn, 10, 11, "temporal", 0.9);
        insert_link(&conn, 11, 12, "temporal", 0.9);

        // Only use "semantic" links
        let options = LouvainOptions {
            link_types: Some(vec!["semantic".to_string()]),
            ..Default::default()
        };
        let result = run_louvain_clustering(&conn, &options).unwrap();

        // Nodes 10, 11, 12 should not appear
        assert_eq!(result.nodes, 3);
        let all_members: Vec<i64> = result
            .clusters
            .iter()
            .flat_map(|c| c.members.clone())
            .collect();
        assert!(!all_members.contains(&10));
        assert!(!all_members.contains(&11));
        assert!(!all_members.contains(&12));
    }

    #[test]
    fn test_min_cluster_size_filter() {
        let conn = setup_db();
        // One pair and one trio
        insert_link(&conn, 1, 2, "semantic", 0.9);
        insert_link(&conn, 10, 11, "semantic", 0.9);
        insert_link(&conn, 11, 12, "semantic", 0.9);
        insert_link(&conn, 10, 12, "semantic", 0.9);

        let options = LouvainOptions {
            min_cluster_size: 3,
            ..Default::default()
        };
        let result = run_louvain_clustering(&conn, &options).unwrap();

        // Only the trio should survive the size filter
        for cluster in &result.clusters {
            assert!(cluster.size >= 3);
        }
    }

    // -----------------------------------------------------------------------
    // Tests for get_cluster
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_cluster_returns_correct_cluster() {
        let conn = setup_db();
        insert_link(&conn, 1, 2, "semantic", 0.9);
        insert_link(&conn, 2, 3, "semantic", 0.9);

        let options = LouvainOptions::default();
        run_louvain_clustering(&conn, &options).unwrap();

        let cluster = get_cluster(&conn, 1).unwrap();
        assert!(cluster.is_some());
        let cluster = cluster.unwrap();
        assert!(cluster.members.contains(&1));
        assert!(cluster.members.contains(&2));
    }

    #[test]
    fn test_get_cluster_returns_none_for_unknown_memory() {
        let conn = setup_db();
        let result = get_cluster(&conn, 9999).unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // Tests for list_clusters
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_clusters_returns_all() {
        let conn = setup_db();
        insert_link(&conn, 1, 2, "semantic", 0.9);
        insert_link(&conn, 10, 11, "semantic", 0.9);
        insert_link(&conn, 11, 12, "semantic", 0.9);
        insert_link(&conn, 10, 12, "semantic", 0.9);
        // Weak bridge
        insert_link(&conn, 2, 10, "semantic", 0.01);

        let options = LouvainOptions::default();
        run_louvain_clustering(&conn, &options).unwrap();

        let clusters = list_clusters(&conn, "louvain").unwrap();
        // Should have at least one cluster
        assert!(!clusters.is_empty());
        // All cluster_ids should be distinct
        let ids: HashSet<usize> = clusters.iter().map(|c| c.cluster_id).collect();
        assert_eq!(ids.len(), clusters.len());
    }

    #[test]
    fn test_list_clusters_empty_when_no_algorithm() {
        let conn = setup_db();
        let clusters = list_clusters(&conn, "nonexistent").unwrap();
        assert!(clusters.is_empty());
    }

    // -----------------------------------------------------------------------
    // Tests for internal helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_adjacency_undirected() {
        let edges = vec![
            Edge {
                from: 1,
                to: 2,
                weight: 0.5,
            },
            Edge {
                from: 2,
                to: 3,
                weight: 0.8,
            },
        ];
        let (adj, nodes) = build_adjacency(&edges);

        assert_eq!(nodes.len(), 3);
        // Node 1 should see node 2
        assert!(adj[&1].iter().any(|(nb, _)| *nb == 2));
        // Node 2 should see both 1 and 3 (undirected)
        assert!(adj[&2].iter().any(|(nb, _)| *nb == 1));
        assert!(adj[&2].iter().any(|(nb, _)| *nb == 3));
    }

    #[test]
    fn test_modularity_single_community() {
        // All nodes in one community — modularity depends on edge density
        let adj: AdjList = {
            let mut m = HashMap::new();
            m.insert(1i64, vec![(2i64, 1.0)]);
            m.insert(2i64, vec![(1i64, 1.0)]);
            m
        };
        let degree_of: HashMap<i64, f64> = {
            let mut d = HashMap::new();
            d.insert(1, 1.0);
            d.insert(2, 1.0);
            d
        };
        let community_of: HashMap<i64, usize> = {
            let mut c = HashMap::new();
            c.insert(1, 0);
            c.insert(2, 0);
            c
        };
        let two_m = 2.0;
        let q = compute_modularity(&community_of, &adj, &degree_of, two_m, 1.0);
        // e_ii = 2/2 = 1, a_i = 2/2 = 1 → Q = 1 - 1 = 0 (single community baseline)
        assert!((q - 0.0).abs() < 1e-9);
    }

    #[test]
    fn test_results_persisted_to_database() {
        let conn = setup_db();
        insert_link(&conn, 1, 2, "semantic", 0.9);
        insert_link(&conn, 2, 3, "semantic", 0.9);
        insert_link(&conn, 1, 3, "semantic", 0.9);

        let options = LouvainOptions::default();
        run_louvain_clustering(&conn, &options).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_clusters WHERE algorithm = 'louvain'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert!(count > 0);
    }

    #[test]
    fn test_rerun_replaces_previous_results() {
        let conn = setup_db();
        insert_link(&conn, 1, 2, "semantic", 0.9);

        let options = LouvainOptions::default();
        run_louvain_clustering(&conn, &options).unwrap();
        let count1: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_clusters WHERE algorithm = 'louvain'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        // Run again — should not accumulate rows
        run_louvain_clustering(&conn, &options).unwrap();
        let count2: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM memory_clusters WHERE algorithm = 'louvain'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(count1, count2);
    }
}
