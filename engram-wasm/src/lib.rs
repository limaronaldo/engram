//! engram-wasm — Engram algorithms compiled to WebAssembly.
//!
//! This crate exposes pure-Rust computational functions from engram-core
//! as wasm-bindgen entry points. No I/O, no SQLite, no tokio.
//!
//! ## Modules
//!
//! - [`bm25`]   — BM25 document scoring
//! - [`tfidf`]  — TF-IDF embedding and cosine similarity
//! - [`graph`]  — BFS traversal and shortest-path on edge lists
//! - [`rrf`]    — Reciprocal Rank Fusion for merging ranked lists
//! - [`entity`] — Regex-based Named Entity Recognition
//!
//! ## Usage from JavaScript
//!
//! ```js
//! import init, { bm25_score, cosine_similarity, extract_entities } from './engram_wasm.js';
//!
//! await init();
//!
//! const score = bm25_score(
//!   JSON.stringify(["rust", "programming"]),  // query terms
//!   JSON.stringify(["rust", "is", "fast"]),   // doc terms
//!   100,    // doc_count
//!   10.0,   // avg_doc_len
//!   1.5,    // k1
//!   0.75,   // b
//! );
//! ```

pub mod bm25;
pub mod entity;
pub mod graph;
pub mod rrf;
pub mod tfidf;

use wasm_bindgen::prelude::*;

// ==============================================================================
// BM25 exports
// ==============================================================================

/// Score a document against query terms using BM25.
///
/// # Arguments (JS)
///
/// * `query_terms_json` — JSON array of query term strings, e.g. `["rust","fast"]`
/// * `doc_terms_json`   — JSON array of document tokens
/// * `doc_count`        — Total documents in corpus
/// * `avg_doc_len`      — Average document length in tokens
/// * `k1`               — BM25 k1 parameter (default 1.5)
/// * `b`                — BM25 b parameter  (default 0.75)
///
/// # Returns
///
/// BM25 relevance score >= 0.0.
#[wasm_bindgen]
pub fn bm25_score(
    query_terms_json: &str,
    doc_terms_json: &str,
    doc_count: usize,
    avg_doc_len: f64,
    k1: f64,
    b: f64,
) -> f64 {
    let Ok(query_terms): Result<Vec<String>, _> = serde_json::from_str(query_terms_json) else {
        return 0.0;
    };
    let Ok(doc_terms): Result<Vec<String>, _> = serde_json::from_str(doc_terms_json) else {
        return 0.0;
    };

    let query_refs: Vec<&str> = query_terms.iter().map(String::as_str).collect();
    let doc_refs: Vec<&str> = doc_terms.iter().map(String::as_str).collect();

    let params = bm25::Bm25Params { k1, b };
    bm25::bm25_score(&query_refs, &doc_refs, doc_count, avg_doc_len, params)
}

/// Tokenize a text string into BM25-compatible lowercase tokens.
///
/// Returns a JSON array of token strings.
#[wasm_bindgen]
pub fn bm25_tokenize(text: &str) -> String {
    let tokens = bm25::tokenize(text);
    serde_json::to_string(&tokens).unwrap_or_else(|_| "[]".to_string())
}

// ==============================================================================
// TF-IDF exports
// ==============================================================================

/// Compute a TF-IDF embedding vector for `text`.
///
/// # Arguments
///
/// * `text`       — Text to embed.
/// * `dimensions` — Output vector size. Pass 0 to use the default (384).
///
/// # Returns
///
/// JSON array of `f32` values, length = `dimensions`.
#[wasm_bindgen]
pub fn tfidf_embed(text: &str, dimensions: usize) -> String {
    let dims = if dimensions == 0 {
        tfidf::DEFAULT_DIMENSIONS
    } else {
        dimensions
    };
    let vec = tfidf::tfidf_embed(text, dims);
    serde_json::to_string(&vec).unwrap_or_else(|_| "[]".to_string())
}

/// Compute cosine similarity between two embedding vectors.
///
/// Both vectors must be JSON arrays of numbers. Returns 0.0 on error.
///
/// # Returns
///
/// Cosine similarity in [-1.0, 1.0]. Returns 0.0 if either vector is all zeros.
#[wasm_bindgen]
pub fn cosine_similarity(vec_a_json: &str, vec_b_json: &str) -> f32 {
    let Ok(a): Result<Vec<f32>, _> = serde_json::from_str(vec_a_json) else {
        return 0.0;
    };
    let Ok(b): Result<Vec<f32>, _> = serde_json::from_str(vec_b_json) else {
        return 0.0;
    };
    tfidf::cosine_similarity(&a, &b)
}

// ==============================================================================
// Graph exports
// ==============================================================================

/// BFS traversal from `start`, up to `max_depth` hops.
///
/// # Arguments
///
/// * `edges_json` — JSON array of `{"from": u64, "to": u64}` objects.
/// * `start`      — Start node ID.
/// * `max_depth`  — Maximum hops (0 = start node only).
///
/// # Returns
///
/// JSON array of `{"node": u64, "depth": usize}` objects in BFS order.
#[wasm_bindgen]
pub fn graph_bfs(edges_json: &str, start: u64, max_depth: usize) -> String {
    let edges = parse_edges(edges_json);
    let result = graph::bfs(&edges, start, max_depth);
    let output: Vec<serde_json::Value> = result
        .into_iter()
        .map(|(node, depth)| serde_json::json!({"node": node, "depth": depth}))
        .collect();
    serde_json::to_string(&output).unwrap_or_else(|_| "[]".to_string())
}

/// Find the shortest undirected path between `start` and `end`.
///
/// # Arguments
///
/// * `edges_json` — JSON array of `{"from": u64, "to": u64}` objects.
/// * `start`      — Source node ID.
/// * `end`        — Target node ID.
///
/// # Returns
///
/// JSON array of node IDs forming the path, or `null` if no path exists.
#[wasm_bindgen]
pub fn graph_shortest_path(edges_json: &str, start: u64, end: u64) -> String {
    let edges = parse_edges(edges_json);
    match graph::shortest_path(&edges, start, end) {
        Some(path) => serde_json::to_string(&path).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    }
}

/// Parse edges from JSON. Returns empty vec on parse error.
fn parse_edges(edges_json: &str) -> Vec<graph::Edge> {
    #[derive(serde::Deserialize)]
    struct RawEdge {
        from: u64,
        to: u64,
    }

    let Ok(raw): Result<Vec<RawEdge>, _> = serde_json::from_str(edges_json) else {
        return Vec::new();
    };

    raw.into_iter().map(|e| graph::Edge::new(e.from, e.to)).collect()
}

// ==============================================================================
// RRF exports
// ==============================================================================

/// Merge multiple ranked lists using Reciprocal Rank Fusion.
///
/// # Arguments
///
/// * `lists_json` — JSON array of ranked lists. Each list is
///   `{"items": [{"doc_id": u64, "rank": usize}], "weight": f64}`.
///   `weight` is optional and defaults to 1.0.
/// * `k`          — RRF constant. Pass 0 to use the default (60.0).
///
/// # Returns
///
/// JSON array of `{"doc_id": u64, "score": f64}` sorted by score descending.
#[wasm_bindgen]
pub fn rrf_merge(lists_json: &str, k: f64) -> String {
    #[derive(serde::Deserialize)]
    struct RawItem {
        doc_id: u64,
        rank: usize,
    }

    #[derive(serde::Deserialize)]
    struct RawList {
        items: Vec<RawItem>,
        #[serde(default = "default_weight")]
        weight: f64,
    }

    fn default_weight() -> f64 {
        1.0
    }

    let Ok(raw): Result<Vec<RawList>, _> = serde_json::from_str(lists_json) else {
        return "[]".to_string();
    };

    let lists: Vec<rrf::RankedList> = raw
        .into_iter()
        .map(|l| {
            rrf::RankedList::with_weight(
                l.items
                    .into_iter()
                    .map(|i| rrf::RankedItem::new(i.doc_id, i.rank))
                    .collect(),
                l.weight,
            )
        })
        .collect();

    let k_val = if k <= 0.0 { rrf::DEFAULT_K } else { k };
    let result = rrf::rrf_merge(&lists, k_val);

    let output: Vec<serde_json::Value> = result
        .into_iter()
        .map(|(doc_id, score)| serde_json::json!({"doc_id": doc_id, "score": score}))
        .collect();

    serde_json::to_string(&output).unwrap_or_else(|_| "[]".to_string())
}

/// Merge keyword and semantic ranked lists (standard hybrid-search pattern).
///
/// # Arguments
///
/// * `keyword_ids_json`  — JSON array of doc IDs in keyword rank order (best first).
/// * `semantic_ids_json` — JSON array of doc IDs in semantic rank order (best first).
/// * `keyword_weight`    — Weight for keyword list (default 1.0).
/// * `semantic_weight`   — Weight for semantic list (default 1.0).
/// * `k`                 — RRF constant (0 = default 60.0).
///
/// # Returns
///
/// JSON array of `{"doc_id": u64, "score": f64}` sorted by score descending.
#[wasm_bindgen]
pub fn rrf_hybrid(
    keyword_ids_json: &str,
    semantic_ids_json: &str,
    keyword_weight: f64,
    semantic_weight: f64,
    k: f64,
) -> String {
    let Ok(keyword_ids): Result<Vec<u64>, _> = serde_json::from_str(keyword_ids_json) else {
        return "[]".to_string();
    };
    let Ok(semantic_ids): Result<Vec<u64>, _> = serde_json::from_str(semantic_ids_json) else {
        return "[]".to_string();
    };

    let kw = if keyword_weight <= 0.0 { 1.0 } else { keyword_weight };
    let sw = if semantic_weight <= 0.0 { 1.0 } else { semantic_weight };
    let k_val = if k <= 0.0 { rrf::DEFAULT_K } else { k };

    let result = rrf::rrf_hybrid(&keyword_ids, &semantic_ids, kw, sw, k_val);

    let output: Vec<serde_json::Value> = result
        .into_iter()
        .map(|(doc_id, score)| serde_json::json!({"doc_id": doc_id, "score": score}))
        .collect();

    serde_json::to_string(&output).unwrap_or_else(|_| "[]".to_string())
}

// ==============================================================================
// Entity extraction exports
// ==============================================================================

/// Extract entities from text.
///
/// Returns a JSON array of entity objects:
/// ```json
/// [
///   {
///     "text": "@alice",
///     "normalized": "alice",
///     "entity_type": "mention",
///     "confidence": 0.9,
///     "position": 6,
///     "count": 1
///   }
/// ]
/// ```
///
/// `entity_type` is one of: `"mention"`, `"email"`, `"url"`, `"name"`.
#[wasm_bindgen]
pub fn extract_entities(text: &str) -> String {
    let config = entity::ExtractConfig::default();
    let entities = entity::extract_entities(text, &config);
    serde_json::to_string(&entities).unwrap_or_else(|_| "[]".to_string())
}

/// Extract entities with a custom maximum count.
#[wasm_bindgen]
pub fn extract_entities_limited(text: &str, max_entities: usize) -> String {
    let config = entity::ExtractConfig {
        max_entities,
        ..Default::default()
    };
    let entities = entity::extract_entities(text, &config);
    serde_json::to_string(&entities).unwrap_or_else(|_| "[]".to_string())
}

// ==============================================================================
// Utility
// ==============================================================================

/// Return the engram-wasm version string.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ==============================================================================
// Tests
// ==============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bm25_score_export() {
        let score = bm25_score(
            r#"["rust","fast"]"#,
            r#"["rust","is","a","fast","language"]"#,
            100,
            10.0,
            1.5,
            0.75,
        );
        assert!(score > 0.0);
    }

    #[test]
    fn test_bm25_score_invalid_json() {
        let score = bm25_score("not json", "[]", 10, 5.0, 1.5, 0.75);
        assert_eq!(score, 0.0);
    }

    #[test]
    fn test_bm25_tokenize_export() {
        let json = bm25_tokenize("Hello World");
        let tokens: Vec<String> = serde_json::from_str(&json).unwrap();
        assert!(tokens.contains(&"hello".to_string()));
        assert!(tokens.contains(&"world".to_string()));
    }

    #[test]
    fn test_tfidf_embed_export() {
        let json = tfidf_embed("rust programming", 128);
        let vec: Vec<f32> = serde_json::from_str(&json).unwrap();
        assert_eq!(vec.len(), 128);
    }

    #[test]
    fn test_tfidf_embed_default_dims() {
        let json = tfidf_embed("some text", 0);
        let vec: Vec<f32> = serde_json::from_str(&json).unwrap();
        assert_eq!(vec.len(), tfidf::DEFAULT_DIMENSIONS);
    }

    #[test]
    fn test_cosine_similarity_export() {
        let a = tfidf_embed("hello world", 64);
        let sim = cosine_similarity(&a, &a);
        assert!((sim - 1.0).abs() < 1e-4);
    }

    #[test]
    fn test_cosine_similarity_invalid_json() {
        assert_eq!(cosine_similarity("not json", "[]"), 0.0);
    }

    #[test]
    fn test_graph_bfs_export() {
        let edges = r#"[{"from":1,"to":2},{"from":2,"to":3}]"#;
        let json = graph_bfs(edges, 1, 10);
        let result: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_graph_shortest_path_export() {
        let edges = r#"[{"from":1,"to":2},{"from":2,"to":3}]"#;
        let json = graph_shortest_path(edges, 1, 3);
        let path: Vec<u64> = serde_json::from_str(&json).unwrap();
        assert_eq!(path, vec![1, 2, 3]);
    }

    #[test]
    fn test_graph_shortest_path_no_path() {
        let edges = r#"[{"from":1,"to":2}]"#;
        let result = graph_shortest_path(edges, 1, 9);
        assert_eq!(result, "null");
    }

    #[test]
    fn test_rrf_hybrid_export() {
        let keyword = r#"[1,2,3]"#;
        let semantic = r#"[3,1,2]"#;
        let json = rrf_hybrid(keyword, semantic, 1.0, 1.0, 0.0);
        let result: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_extract_entities_export() {
        let json = extract_entities("Email me at bob@example.com or @alice");
        let entities: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
        assert!(!entities.is_empty());
    }

    #[test]
    fn test_version_export() {
        let v = version();
        assert!(!v.is_empty());
    }
}
