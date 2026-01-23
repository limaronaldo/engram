//! Knowledge graph visualization (RML-894 improvements)
//!
//! Provides:
//! - Interactive graph visualization with vis.js
//! - Graph clustering and community detection
//! - Graph statistics and metrics
//! - Export to multiple formats (HTML, DOT, JSON)
//! - Filtering and traversal utilities

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::types::{CrossReference, Memory, MemoryId};

/// Graph node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: MemoryId,
    pub label: String,
    pub memory_type: String,
    pub importance: f32,
    pub tags: Vec<String>,
}

/// Graph edge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: MemoryId,
    pub to: MemoryId,
    pub edge_type: String,
    pub score: f32,
    pub confidence: f32,
}

/// Knowledge graph structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl KnowledgeGraph {
    /// Create graph from memories and cross-references
    pub fn from_data(memories: &[Memory], crossrefs: &[CrossReference]) -> Self {
        let nodes: Vec<GraphNode> = memories
            .iter()
            .map(|m| GraphNode {
                id: m.id,
                label: truncate_label(&m.content, 50),
                memory_type: m.memory_type.as_str().to_string(),
                importance: m.importance,
                tags: m.tags.clone(),
            })
            .collect();

        let memory_ids: std::collections::HashSet<MemoryId> =
            memories.iter().map(|m| m.id).collect();

        let edges: Vec<GraphEdge> = crossrefs
            .iter()
            .filter(|cr| memory_ids.contains(&cr.from_id) && memory_ids.contains(&cr.to_id))
            .map(|cr| GraphEdge {
                from: cr.from_id,
                to: cr.to_id,
                edge_type: cr.edge_type.as_str().to_string(),
                score: cr.score,
                confidence: cr.confidence,
            })
            .collect();

        Self { nodes, edges }
    }

    /// Export as vis.js compatible JSON
    pub fn to_visjs_json(&self) -> serde_json::Value {
        let nodes: Vec<serde_json::Value> = self
            .nodes
            .iter()
            .map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "label": n.label,
                    "group": n.memory_type,
                    "value": (n.importance * 10.0) as i32 + 5,
                    "title": format!("Type: {}\nTags: {}", n.memory_type, n.tags.join(", "))
                })
            })
            .collect();

        let edges: Vec<serde_json::Value> = self
            .edges
            .iter()
            .map(|e| {
                serde_json::json!({
                    "from": e.from,
                    "to": e.to,
                    "label": e.edge_type,
                    "value": (e.score * e.confidence * 5.0) as i32 + 1,
                    "title": format!("Score: {:.2}, Confidence: {:.2}", e.score, e.confidence)
                })
            })
            .collect();

        serde_json::json!({
            "nodes": nodes,
            "edges": edges
        })
    }

    /// Export as standalone HTML with vis.js
    pub fn to_html(&self) -> String {
        let graph_data = self.to_visjs_json();

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <title>Engram Knowledge Graph</title>
    <script type="text/javascript" src="https://unpkg.com/vis-network/standalone/umd/vis-network.min.js"></script>
    <style>
        body {{ margin: 0; padding: 0; font-family: system-ui, sans-serif; }}
        #graph {{ width: 100vw; height: 100vh; }}
        #controls {{
            position: absolute;
            top: 10px;
            left: 10px;
            background: white;
            padding: 10px;
            border-radius: 8px;
            box-shadow: 0 2px 8px rgba(0,0,0,0.1);
        }}
        #search {{ padding: 8px; width: 200px; border: 1px solid #ddd; border-radius: 4px; }}
        .legend {{ display: flex; gap: 10px; margin-top: 10px; flex-wrap: wrap; }}
        .legend-item {{ display: flex; align-items: center; gap: 5px; font-size: 12px; }}
        .legend-dot {{ width: 12px; height: 12px; border-radius: 50%; }}
    </style>
</head>
<body>
    <div id="controls">
        <input type="text" id="search" placeholder="Search nodes...">
        <div class="legend">
            <div class="legend-item"><span class="legend-dot" style="background: #97C2FC;"></span> note</div>
            <div class="legend-item"><span class="legend-dot" style="background: #FFFF00;"></span> todo</div>
            <div class="legend-item"><span class="legend-dot" style="background: #FB7E81;"></span> issue</div>
            <div class="legend-item"><span class="legend-dot" style="background: #7BE141;"></span> decision</div>
            <div class="legend-item"><span class="legend-dot" style="background: #FFA807;"></span> preference</div>
            <div class="legend-item"><span class="legend-dot" style="background: #6E6EFD;"></span> learning</div>
        </div>
    </div>
    <div id="graph"></div>
    <script>
        const data = {graph_data};

        const options = {{
            nodes: {{
                shape: 'dot',
                scaling: {{ min: 10, max: 30 }},
                font: {{ size: 12, face: 'system-ui' }}
            }},
            edges: {{
                arrows: 'to',
                scaling: {{ min: 1, max: 5 }},
                font: {{ size: 10, align: 'middle' }}
            }},
            groups: {{
                note: {{ color: '#97C2FC' }},
                todo: {{ color: '#FFFF00' }},
                issue: {{ color: '#FB7E81' }},
                decision: {{ color: '#7BE141' }},
                preference: {{ color: '#FFA807' }},
                learning: {{ color: '#6E6EFD' }},
                context: {{ color: '#C2FABC' }},
                credential: {{ color: '#FD6A6A' }}
            }},
            physics: {{
                stabilization: {{ iterations: 100 }},
                barnesHut: {{
                    gravitationalConstant: -2000,
                    springLength: 100
                }}
            }},
            interaction: {{
                hover: true,
                tooltipDelay: 100
            }}
        }};

        const container = document.getElementById('graph');
        const network = new vis.Network(container, data, options);

        // Search functionality
        const searchInput = document.getElementById('search');
        searchInput.addEventListener('input', function() {{
            const query = this.value.toLowerCase();
            if (query) {{
                const matchingNodes = data.nodes.filter(n =>
                    n.label.toLowerCase().includes(query)
                ).map(n => n.id);
                network.selectNodes(matchingNodes);
                if (matchingNodes.length > 0) {{
                    network.focus(matchingNodes[0], {{ scale: 1.5, animation: true }});
                }}
            }} else {{
                network.unselectAll();
            }}
        }});

        // Click to focus
        network.on('click', function(params) {{
            if (params.nodes.length > 0) {{
                network.focus(params.nodes[0], {{ scale: 1.5, animation: true }});
            }}
        }});
    </script>
</body>
</html>"#,
            graph_data = serde_json::to_string(&graph_data).unwrap_or_default()
        )
    }
}

/// Truncate content for display as node label
fn truncate_label(content: &str, max_len: usize) -> String {
    let first_line = content.lines().next().unwrap_or(content);
    if first_line.len() <= max_len {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..max_len - 3])
    }
}

// =============================================================================
// Graph Statistics (RML-894)
// =============================================================================

/// Graph statistics and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    /// Total number of nodes
    pub node_count: usize,
    /// Total number of edges
    pub edge_count: usize,
    /// Average degree (edges per node)
    pub avg_degree: f32,
    /// Graph density (actual edges / possible edges)
    pub density: f32,
    /// Number of connected components
    pub component_count: usize,
    /// Size of largest component
    pub largest_component_size: usize,
    /// Nodes by memory type
    pub nodes_by_type: HashMap<String, usize>,
    /// Edges by type
    pub edges_by_type: HashMap<String, usize>,
    /// Most connected nodes (top 10 by degree)
    pub hub_nodes: Vec<(MemoryId, usize)>,
    /// Isolated nodes (degree 0)
    pub isolated_count: usize,
}

impl KnowledgeGraph {
    /// Calculate graph statistics
    pub fn stats(&self) -> GraphStats {
        let node_count = self.nodes.len();
        let edge_count = self.edges.len();

        // Build adjacency for degree calculation
        let mut degree: HashMap<MemoryId, usize> = HashMap::new();
        for node in &self.nodes {
            degree.insert(node.id, 0);
        }
        for edge in &self.edges {
            *degree.entry(edge.from).or_insert(0) += 1;
            *degree.entry(edge.to).or_insert(0) += 1;
        }

        let avg_degree = if node_count > 0 {
            degree.values().sum::<usize>() as f32 / node_count as f32
        } else {
            0.0
        };

        // Density: edges / (n * (n-1) / 2) for undirected, edges / (n * (n-1)) for directed
        let density = if node_count > 1 {
            edge_count as f32 / (node_count * (node_count - 1)) as f32
        } else {
            0.0
        };

        // Count by type
        let mut nodes_by_type: HashMap<String, usize> = HashMap::new();
        for node in &self.nodes {
            *nodes_by_type.entry(node.memory_type.clone()).or_insert(0) += 1;
        }

        let mut edges_by_type: HashMap<String, usize> = HashMap::new();
        for edge in &self.edges {
            *edges_by_type.entry(edge.edge_type.clone()).or_insert(0) += 1;
        }

        // Find hub nodes (top 10 by degree)
        let mut degree_list: Vec<(MemoryId, usize)> =
            degree.iter().map(|(&k, &v)| (k, v)).collect();
        degree_list.sort_by(|a, b| b.1.cmp(&a.1));
        let hub_nodes: Vec<(MemoryId, usize)> = degree_list.into_iter().take(10).collect();

        // Count isolated nodes
        let isolated_count = degree.values().filter(|&&d| d == 0).count();

        // Find connected components using BFS
        let components = self.find_connected_components();
        let component_count = components.len();
        let largest_component_size = components.iter().map(|c| c.len()).max().unwrap_or(0);

        GraphStats {
            node_count,
            edge_count,
            avg_degree,
            density,
            component_count,
            largest_component_size,
            nodes_by_type,
            edges_by_type,
            hub_nodes,
            isolated_count,
        }
    }

    /// Find connected components using BFS
    fn find_connected_components(&self) -> Vec<Vec<MemoryId>> {
        let node_ids: HashSet<MemoryId> = self.nodes.iter().map(|n| n.id).collect();

        // Build adjacency list (undirected)
        let mut adj: HashMap<MemoryId, Vec<MemoryId>> = HashMap::new();
        for id in &node_ids {
            adj.insert(*id, Vec::new());
        }
        for edge in &self.edges {
            if let Some(list) = adj.get_mut(&edge.from) {
                list.push(edge.to);
            }
            if let Some(list) = adj.get_mut(&edge.to) {
                list.push(edge.from);
            }
        }

        let mut visited: HashSet<MemoryId> = HashSet::new();
        let mut components = Vec::new();

        for &start in &node_ids {
            if visited.contains(&start) {
                continue;
            }

            let mut component = Vec::new();
            let mut queue = VecDeque::new();
            queue.push_back(start);
            visited.insert(start);

            while let Some(node) = queue.pop_front() {
                component.push(node);
                if let Some(neighbors) = adj.get(&node) {
                    for &neighbor in neighbors {
                        if !visited.contains(&neighbor) {
                            visited.insert(neighbor);
                            queue.push_back(neighbor);
                        }
                    }
                }
            }

            components.push(component);
        }

        components
    }

    /// Calculate centrality scores for nodes
    pub fn centrality(&self) -> HashMap<MemoryId, CentralityScores> {
        let mut results: HashMap<MemoryId, CentralityScores> = HashMap::new();

        // Build adjacency
        let mut in_degree: HashMap<MemoryId, usize> = HashMap::new();
        let mut out_degree: HashMap<MemoryId, usize> = HashMap::new();

        for node in &self.nodes {
            in_degree.insert(node.id, 0);
            out_degree.insert(node.id, 0);
        }

        for edge in &self.edges {
            *out_degree.entry(edge.from).or_insert(0) += 1;
            *in_degree.entry(edge.to).or_insert(0) += 1;
        }

        let max_degree = self.nodes.len().saturating_sub(1).max(1) as f32;

        for node in &self.nodes {
            let in_d = *in_degree.get(&node.id).unwrap_or(&0) as f32;
            let out_d = *out_degree.get(&node.id).unwrap_or(&0) as f32;

            results.insert(
                node.id,
                CentralityScores {
                    in_degree: in_d / max_degree,
                    out_degree: out_d / max_degree,
                    degree: (in_d + out_d) / (2.0 * max_degree),
                    // Simplified closeness based on direct connections
                    closeness: (in_d + out_d) / (2.0 * max_degree),
                },
            );
        }

        results
    }
}

/// Centrality scores for a node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CentralityScores {
    /// Normalized in-degree centrality
    pub in_degree: f32,
    /// Normalized out-degree centrality
    pub out_degree: f32,
    /// Combined degree centrality
    pub degree: f32,
    /// Closeness centrality (simplified)
    pub closeness: f32,
}

// =============================================================================
// Graph Filtering (RML-894)
// =============================================================================

/// Filter options for graph queries
#[derive(Debug, Clone, Default)]
pub struct GraphFilter {
    /// Filter by memory types
    pub memory_types: Option<Vec<String>>,
    /// Filter by tags (any match)
    pub tags: Option<Vec<String>>,
    /// Filter by edge types
    pub edge_types: Option<Vec<String>>,
    /// Minimum importance threshold
    pub min_importance: Option<f32>,
    /// Maximum importance threshold
    pub max_importance: Option<f32>,
    /// Created after this date
    pub created_after: Option<DateTime<Utc>>,
    /// Created before this date
    pub created_before: Option<DateTime<Utc>>,
    /// Minimum edge confidence
    pub min_confidence: Option<f32>,
    /// Minimum edge score
    pub min_score: Option<f32>,
    /// Maximum number of nodes
    pub limit: Option<usize>,
}

impl GraphFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_types(mut self, types: Vec<String>) -> Self {
        self.memory_types = Some(types);
        self
    }

    pub fn with_tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn with_min_importance(mut self, min: f32) -> Self {
        self.min_importance = Some(min);
        self
    }

    pub fn with_min_confidence(mut self, min: f32) -> Self {
        self.min_confidence = Some(min);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

impl KnowledgeGraph {
    /// Apply filter to create a subgraph
    pub fn filter(&self, filter: &GraphFilter) -> KnowledgeGraph {
        // Filter nodes
        let mut filtered_nodes: Vec<GraphNode> = self
            .nodes
            .iter()
            .filter(|n| {
                // Type filter
                if let Some(ref types) = filter.memory_types {
                    if !types.contains(&n.memory_type) {
                        return false;
                    }
                }

                // Tag filter (any match)
                if let Some(ref tags) = filter.tags {
                    if !n.tags.iter().any(|t| tags.contains(t)) {
                        return false;
                    }
                }

                // Importance filter
                if let Some(min) = filter.min_importance {
                    if n.importance < min {
                        return false;
                    }
                }
                if let Some(max) = filter.max_importance {
                    if n.importance > max {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        // Apply limit
        if let Some(limit) = filter.limit {
            filtered_nodes.truncate(limit);
        }

        // Get set of valid node IDs
        let valid_ids: HashSet<MemoryId> = filtered_nodes.iter().map(|n| n.id).collect();

        // Filter edges
        let filtered_edges: Vec<GraphEdge> = self
            .edges
            .iter()
            .filter(|e| {
                // Both endpoints must be in filtered nodes
                if !valid_ids.contains(&e.from) || !valid_ids.contains(&e.to) {
                    return false;
                }

                // Edge type filter
                if let Some(ref types) = filter.edge_types {
                    if !types.contains(&e.edge_type) {
                        return false;
                    }
                }

                // Confidence filter
                if let Some(min) = filter.min_confidence {
                    if e.confidence < min {
                        return false;
                    }
                }

                // Score filter
                if let Some(min) = filter.min_score {
                    if e.score < min {
                        return false;
                    }
                }

                true
            })
            .cloned()
            .collect();

        KnowledgeGraph {
            nodes: filtered_nodes,
            edges: filtered_edges,
        }
    }

    /// Get subgraph centered on a node with given depth
    pub fn neighborhood(&self, center: MemoryId, depth: usize) -> KnowledgeGraph {
        let mut visited: HashSet<MemoryId> = HashSet::new();
        let mut current_level: HashSet<MemoryId> = HashSet::new();
        current_level.insert(center);
        visited.insert(center);

        // Build adjacency
        let mut adj: HashMap<MemoryId, Vec<MemoryId>> = HashMap::new();
        for edge in &self.edges {
            adj.entry(edge.from).or_default().push(edge.to);
            adj.entry(edge.to).or_default().push(edge.from);
        }

        // BFS to depth
        for _ in 0..depth {
            let mut next_level: HashSet<MemoryId> = HashSet::new();
            for &node in &current_level {
                if let Some(neighbors) = adj.get(&node) {
                    for &neighbor in neighbors {
                        if !visited.contains(&neighbor) {
                            visited.insert(neighbor);
                            next_level.insert(neighbor);
                        }
                    }
                }
            }
            current_level = next_level;
        }

        // Filter to visited nodes
        let nodes: Vec<GraphNode> = self
            .nodes
            .iter()
            .filter(|n| visited.contains(&n.id))
            .cloned()
            .collect();

        let edges: Vec<GraphEdge> = self
            .edges
            .iter()
            .filter(|e| visited.contains(&e.from) && visited.contains(&e.to))
            .cloned()
            .collect();

        KnowledgeGraph { nodes, edges }
    }
}

// =============================================================================
// DOT Export (RML-894)
// =============================================================================

impl KnowledgeGraph {
    /// Export as DOT format for Graphviz
    pub fn to_dot(&self) -> String {
        let mut dot = String::from("digraph knowledge_graph {\n");
        dot.push_str("    rankdir=LR;\n");
        dot.push_str("    node [shape=box, style=rounded];\n\n");

        // Color mapping for memory types
        let colors: HashMap<&str, &str> = [
            ("note", "#97C2FC"),
            ("todo", "#FFFF00"),
            ("issue", "#FB7E81"),
            ("decision", "#7BE141"),
            ("preference", "#FFA807"),
            ("learning", "#6E6EFD"),
            ("context", "#C2FABC"),
            ("credential", "#FD6A6A"),
        ]
        .into_iter()
        .collect();

        // Write nodes
        for node in &self.nodes {
            let color = colors.get(node.memory_type.as_str()).unwrap_or(&"#CCCCCC");
            let label = node.label.replace('"', "\\\"");
            dot.push_str(&format!(
                "    \"{}\" [label=\"{}\", fillcolor=\"{}\", style=\"filled,rounded\"];\n",
                node.id, label, color
            ));
        }

        dot.push('\n');

        // Write edges
        for edge in &self.edges {
            let style = match edge.edge_type.as_str() {
                "related_to" => "solid",
                "part_of" => "dashed",
                "depends_on" => "bold",
                "contradicts" => "dotted",
                "supports" => "solid",
                "references" => "dashed",
                _ => "solid",
            };
            dot.push_str(&format!(
                "    \"{}\" -> \"{}\" [label=\"{}\", style={}, penwidth={}];\n",
                edge.from,
                edge.to,
                edge.edge_type,
                style,
                (edge.score * 2.0 + 0.5).min(3.0)
            ));
        }

        dot.push_str("}\n");
        dot
    }

    /// Export as GEXF format for Gephi
    pub fn to_gexf(&self) -> String {
        let mut gexf = String::from(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<gexf xmlns="http://gexf.net/1.3" version="1.3">
  <meta>
    <creator>Engram</creator>
    <description>Knowledge Graph Export</description>
  </meta>
  <graph mode="static" defaultedgetype="directed">
    <attributes class="node">
      <attribute id="0" title="type" type="string"/>
      <attribute id="1" title="importance" type="float"/>
    </attributes>
    <attributes class="edge">
      <attribute id="0" title="score" type="float"/>
      <attribute id="1" title="confidence" type="float"/>
    </attributes>
    <nodes>
"#,
        );

        for node in &self.nodes {
            let label = node
                .label
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;");
            gexf.push_str(&format!(
                r#"      <node id="{}" label="{}">
        <attvalues>
          <attvalue for="0" value="{}"/>
          <attvalue for="1" value="{}"/>
        </attvalues>
      </node>
"#,
                node.id, label, node.memory_type, node.importance
            ));
        }

        gexf.push_str("    </nodes>\n    <edges>\n");

        for (i, edge) in self.edges.iter().enumerate() {
            gexf.push_str(&format!(
                r#"      <edge id="{}" source="{}" target="{}" label="{}">
        <attvalues>
          <attvalue for="0" value="{}"/>
          <attvalue for="1" value="{}"/>
        </attvalues>
      </edge>
"#,
                i, edge.from, edge.to, edge.edge_type, edge.score, edge.confidence
            ));
        }

        gexf.push_str("    </edges>\n  </graph>\n</gexf>\n");
        gexf
    }
}

// =============================================================================
// Community Detection (RML-894)
// =============================================================================

/// A cluster/community of nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphCluster {
    /// Cluster identifier
    pub id: usize,
    /// Node IDs in this cluster
    pub members: Vec<MemoryId>,
    /// Dominant memory type in cluster
    pub dominant_type: Option<String>,
    /// Common tags across cluster
    pub common_tags: Vec<String>,
    /// Internal edge count
    pub internal_edges: usize,
    /// Cluster cohesion score
    pub cohesion: f32,
}

impl KnowledgeGraph {
    /// Detect communities using label propagation algorithm
    pub fn detect_communities(&self, max_iterations: usize) -> Vec<GraphCluster> {
        if self.nodes.is_empty() {
            return Vec::new();
        }

        // Initialize: each node in its own community
        let mut labels: HashMap<MemoryId, usize> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.id, i))
            .collect();

        // Build adjacency
        let mut adj: HashMap<MemoryId, Vec<(MemoryId, f32)>> = HashMap::new();
        for node in &self.nodes {
            adj.insert(node.id, Vec::new());
        }
        for edge in &self.edges {
            let weight = edge.score * edge.confidence;
            adj.entry(edge.from).or_default().push((edge.to, weight));
            adj.entry(edge.to).or_default().push((edge.from, weight));
        }

        // Label propagation
        let node_ids: Vec<MemoryId> = self.nodes.iter().map(|n| n.id).collect();

        for _ in 0..max_iterations {
            let mut changed = false;

            for &node_id in &node_ids {
                if let Some(neighbors) = adj.get(&node_id) {
                    if neighbors.is_empty() {
                        continue;
                    }

                    // Count weighted votes for each label
                    let mut votes: HashMap<usize, f32> = HashMap::new();
                    for &(neighbor, weight) in neighbors {
                        if let Some(&label) = labels.get(&neighbor) {
                            *votes.entry(label).or_insert(0.0) += weight;
                        }
                    }

                    // Pick label with most votes
                    if let Some((&best_label, _)) = votes.iter().max_by(|a, b| a.1.total_cmp(b.1)) {
                        let current = labels.get(&node_id).copied().unwrap_or(0);
                        if best_label != current {
                            labels.insert(node_id, best_label);
                            changed = true;
                        }
                    }
                }
            }

            if !changed {
                break;
            }
        }

        // Group nodes by label
        let mut clusters_map: HashMap<usize, Vec<MemoryId>> = HashMap::new();
        for (node_id, label) in &labels {
            clusters_map.entry(*label).or_default().push(*node_id);
        }

        // Build cluster objects
        let node_map: HashMap<MemoryId, &GraphNode> =
            self.nodes.iter().map(|n| (n.id, n)).collect();

        let mut clusters: Vec<GraphCluster> = clusters_map
            .into_iter()
            .enumerate()
            .map(|(new_id, (_, members))| {
                // Find dominant type
                let mut type_counts: HashMap<String, usize> = HashMap::new();
                let mut all_tags: HashMap<String, usize> = HashMap::new();

                for &member_id in &members {
                    if let Some(node) = node_map.get(&member_id) {
                        *type_counts.entry(node.memory_type.clone()).or_insert(0) += 1;
                        for tag in &node.tags {
                            *all_tags.entry(tag.clone()).or_insert(0) += 1;
                        }
                    }
                }

                let dominant_type = type_counts
                    .iter()
                    .max_by_key(|(_, count)| *count)
                    .map(|(t, _)| t.clone());

                // Common tags (present in > 50% of members)
                let threshold = members.len() / 2;
                let common_tags: Vec<String> = all_tags
                    .into_iter()
                    .filter(|(_, count)| *count > threshold)
                    .map(|(tag, _)| tag)
                    .collect();

                // Count internal edges
                let member_set: HashSet<MemoryId> = members.iter().copied().collect();
                let internal_edges = self
                    .edges
                    .iter()
                    .filter(|e| member_set.contains(&e.from) && member_set.contains(&e.to))
                    .count();

                // Cohesion: internal edges / possible internal edges
                let n = members.len();
                let possible = if n > 1 { n * (n - 1) } else { 1 };
                let cohesion = internal_edges as f32 / possible as f32;

                GraphCluster {
                    id: new_id,
                    members,
                    dominant_type,
                    common_tags,
                    internal_edges,
                    cohesion,
                }
            })
            .collect();

        // Sort by size (largest first)
        clusters.sort_by(|a, b| b.members.len().cmp(&a.members.len()));

        // Renumber IDs
        for (i, cluster) in clusters.iter_mut().enumerate() {
            cluster.id = i;
        }

        clusters
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: MemoryId, memory_type: &str, tags: Vec<&str>) -> GraphNode {
        GraphNode {
            id,
            label: format!("Node {}", id),
            memory_type: memory_type.to_string(),
            importance: 0.5,
            tags: tags.into_iter().map(String::from).collect(),
        }
    }

    fn make_edge(from: MemoryId, to: MemoryId, edge_type: &str) -> GraphEdge {
        GraphEdge {
            from,
            to,
            edge_type: edge_type.to_string(),
            score: 0.8,
            confidence: 0.9,
        }
    }

    #[test]
    fn test_truncate_label() {
        assert_eq!(truncate_label("short", 50), "short");
        assert_eq!(
            truncate_label("this is a very long label that should be truncated", 20),
            "this is a very lo..."
        );
    }

    #[test]
    fn test_graph_stats() {
        let id1: MemoryId = 1;
        let id2: MemoryId = 2;
        let id3: MemoryId = 3;

        let graph = KnowledgeGraph {
            nodes: vec![
                make_node(id1, "note", vec!["rust"]),
                make_node(id2, "note", vec!["rust"]),
                make_node(id3, "todo", vec!["python"]),
            ],
            edges: vec![
                make_edge(id1, id2, "related_to"),
                make_edge(id2, id3, "depends_on"),
            ],
        };

        let stats = graph.stats();
        assert_eq!(stats.node_count, 3);
        assert_eq!(stats.edge_count, 2);
        assert_eq!(stats.nodes_by_type.get("note"), Some(&2));
        assert_eq!(stats.nodes_by_type.get("todo"), Some(&1));
        assert_eq!(stats.isolated_count, 0);
        assert_eq!(stats.component_count, 1);
    }

    #[test]
    fn test_graph_filter() {
        let id1: MemoryId = 1;
        let id2: MemoryId = 2;
        let id3: MemoryId = 3;

        let graph = KnowledgeGraph {
            nodes: vec![
                make_node(id1, "note", vec!["rust"]),
                make_node(id2, "note", vec!["python"]),
                make_node(id3, "todo", vec!["rust"]),
            ],
            edges: vec![
                make_edge(id1, id2, "related_to"),
                make_edge(id2, id3, "depends_on"),
            ],
        };

        // Filter by type
        let filter = GraphFilter::new().with_types(vec!["note".to_string()]);
        let filtered = graph.filter(&filter);
        assert_eq!(filtered.nodes.len(), 2);
        assert_eq!(filtered.edges.len(), 1); // Only edge between notes

        // Filter by tag
        let filter = GraphFilter::new().with_tags(vec!["rust".to_string()]);
        let filtered = graph.filter(&filter);
        assert_eq!(filtered.nodes.len(), 2); // id1 and id3 have "rust"
    }

    #[test]
    fn test_neighborhood() {
        let id1: MemoryId = 1;
        let id2: MemoryId = 2;
        let id3: MemoryId = 3;
        let id4: MemoryId = 4;

        let graph = KnowledgeGraph {
            nodes: vec![
                make_node(id1, "note", vec![]),
                make_node(id2, "note", vec![]),
                make_node(id3, "note", vec![]),
                make_node(id4, "note", vec![]),
            ],
            edges: vec![
                make_edge(id1, id2, "related_to"),
                make_edge(id2, id3, "related_to"),
                make_edge(id3, id4, "related_to"),
            ],
        };

        // Depth 1 from id1 should include id1, id2
        let subgraph = graph.neighborhood(id1, 1);
        assert_eq!(subgraph.nodes.len(), 2);

        // Depth 2 from id1 should include id1, id2, id3
        let subgraph = graph.neighborhood(id1, 2);
        assert_eq!(subgraph.nodes.len(), 3);
    }

    #[test]
    fn test_to_dot() {
        let id1: MemoryId = 1;
        let id2: MemoryId = 2;

        let graph = KnowledgeGraph {
            nodes: vec![
                make_node(id1, "note", vec![]),
                make_node(id2, "todo", vec![]),
            ],
            edges: vec![make_edge(id1, id2, "related_to")],
        };

        let dot = graph.to_dot();
        assert!(dot.contains("digraph knowledge_graph"));
        assert!(dot.contains(&id1.to_string()));
        assert!(dot.contains(&id2.to_string()));
        assert!(dot.contains("related_to"));
    }

    #[test]
    fn test_community_detection() {
        // Create two clusters
        let a1: MemoryId = 1;
        let a2: MemoryId = 2;
        let a3: MemoryId = 3;
        let b1: MemoryId = 4;
        let b2: MemoryId = 5;

        let graph = KnowledgeGraph {
            nodes: vec![
                make_node(a1, "note", vec!["cluster-a"]),
                make_node(a2, "note", vec!["cluster-a"]),
                make_node(a3, "note", vec!["cluster-a"]),
                make_node(b1, "todo", vec!["cluster-b"]),
                make_node(b2, "todo", vec!["cluster-b"]),
            ],
            edges: vec![
                // Cluster A - densely connected
                make_edge(a1, a2, "related_to"),
                make_edge(a2, a3, "related_to"),
                make_edge(a1, a3, "related_to"),
                // Cluster B - connected
                make_edge(b1, b2, "related_to"),
                // Weak link between clusters
                GraphEdge {
                    from: a3,
                    to: b1,
                    edge_type: "related_to".to_string(),
                    score: 0.1, // weak
                    confidence: 0.1,
                },
            ],
        };

        let communities = graph.detect_communities(10);
        // Should detect at least the general structure
        assert!(!communities.is_empty());
        // Largest community should have at least 2 members
        assert!(communities[0].members.len() >= 2);
    }
}
