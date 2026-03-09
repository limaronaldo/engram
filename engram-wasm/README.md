# engram-wasm

Engram algorithms compiled to WebAssembly.

Pure-Rust computational functions from engram-core, with no I/O dependencies
(no SQLite, no tokio, no HTTP). Suitable for running in browsers, edge workers,
and any WASM runtime.

## Algorithms

| Module | Functions | Description |
|--------|-----------|-------------|
| `bm25` | `bm25_score`, `bm25_tokenize` | BM25 document scoring |
| `tfidf` | `tfidf_embed`, `cosine_similarity` | TF-IDF embedding + cosine similarity |
| `graph` | `graph_bfs`, `graph_shortest_path` | BFS traversal and shortest-path |
| `rrf` | `rrf_merge`, `rrf_hybrid` | Reciprocal Rank Fusion |
| `entity` | `extract_entities`, `extract_entities_limited` | Regex-based NER |

## Building

```bash
# Check that it compiles (native)
cargo check

# Run all tests (native)
cargo test

# Compile to WASM (requires wasm-pack)
wasm-pack build --target web
wasm-pack build --target nodejs
```

## Usage from JavaScript

```js
import init, {
  bm25_score,
  bm25_tokenize,
  tfidf_embed,
  cosine_similarity,
  graph_bfs,
  graph_shortest_path,
  rrf_hybrid,
  extract_entities,
} from './engram_wasm.js';

await init();

// BM25 scoring
const queryTokens = bm25_tokenize("rust programming");
const docTokens   = bm25_tokenize("Rust is a fast systems programming language");
const score = bm25_score(queryTokens, docTokens, 1000, 50.0, 1.5, 0.75);
console.log("BM25 score:", score);

// TF-IDF similarity
const vecA = tfidf_embed("the quick brown fox", 384);
const vecB = tfidf_embed("a fast brown fox", 384);
const sim  = cosine_similarity(vecA, vecB);
console.log("Cosine similarity:", sim);

// BFS graph traversal
const edges = JSON.stringify([{from: 1, to: 2}, {from: 2, to: 3}]);
const visited = graph_bfs(edges, 1, 5);
console.log("BFS:", JSON.parse(visited));

// Shortest path
const path = graph_shortest_path(edges, 1, 3);
console.log("Path:", JSON.parse(path)); // [1, 2, 3]

// Hybrid search merge (keyword + semantic)
const keywordRanked  = JSON.stringify([10, 20, 30]);
const semanticRanked = JSON.stringify([30, 10, 20]);
const merged = rrf_hybrid(keywordRanked, semanticRanked, 1.0, 1.0, 60.0);
console.log("RRF merged:", JSON.parse(merged));

// Entity extraction
const entities = extract_entities("Email @alice at alice@example.com");
console.log("Entities:", JSON.parse(entities));
```

## API Reference

### BM25

#### `bm25_score(query_terms_json, doc_terms_json, doc_count, avg_doc_len, k1, b) -> f64`

Score a document against query terms. Both term arrays are JSON strings
(`'["rust","fast"]'`). Returns BM25 relevance score >= 0.0.

- `k1` — term saturation (default 1.5, range 1.2–2.0)
- `b`  — length normalization (default 0.75, range 0.5–0.8)

#### `bm25_tokenize(text) -> string`

Tokenize text into lowercase alphanumeric tokens. Returns a JSON array.

### TF-IDF

#### `tfidf_embed(text, dimensions) -> string`

Produce a TF-IDF embedding vector. Returns a JSON array of `f32` values.
Pass `dimensions = 0` to use the default (384).

The algorithm uses feature hashing with bigrams — no vocabulary required,
fully deterministic across all platforms.

#### `cosine_similarity(vec_a_json, vec_b_json) -> f32`

Cosine similarity between two embedding vectors. Returns a value in [-1.0, 1.0].

### Graph

#### `graph_bfs(edges_json, start, max_depth) -> string`

BFS traversal from `start`. `edges_json` is a JSON array of
`{"from": number, "to": number}` objects. Returns JSON array of
`{"node": number, "depth": number}` in BFS order.

#### `graph_shortest_path(edges_json, start, end) -> string`

Shortest undirected path. Returns JSON array of node IDs, or `"null"` if no path.

### RRF

#### `rrf_hybrid(keyword_ids_json, semantic_ids_json, keyword_weight, semantic_weight, k) -> string`

Standard hybrid search merge (keyword + semantic). Both inputs are JSON arrays
of doc IDs in rank order (index 0 = best). Returns
`[{"doc_id": number, "score": number}]` sorted by score descending.

Pass `k = 0` to use the default (60.0).

#### `rrf_merge(lists_json, k) -> string`

General RRF merge for any number of ranked lists. Input format:
```json
[
  {"items": [{"doc_id": 1, "rank": 1}, {"doc_id": 2, "rank": 2}], "weight": 1.0},
  {"items": [{"doc_id": 2, "rank": 1}, {"doc_id": 1, "rank": 2}], "weight": 0.5}
]
```

### Entity Extraction

#### `extract_entities(text) -> string`

Extract entities from text. Returns JSON array of:
```json
[
  {
    "text": "@alice",
    "normalized": "alice",
    "entity_type": "mention",
    "confidence": 0.9,
    "position": 6,
    "count": 1
  }
]
```

Entity types: `"mention"`, `"email"`, `"url"`, `"name"`.

#### `extract_entities_limited(text, max_entities) -> string`

Same as `extract_entities` with a custom upper bound on result count.

### Utility

#### `version() -> string`

Returns the crate version string.

## Design Notes

- All functions accept and return strings (JSON-encoded) to work cleanly
  across the WASM boundary without complex type marshalling.
- The TF-IDF embedder uses the feature-hashing trick — no vocabulary file
  required. Embeddings are deterministic and L2-normalized.
- Graph traversal treats all edges as undirected (matching engram-core
  `neighborhood` and `find_connected_components`).
- RRF uses the standard constant `k = 60` from Cormack et al. (2009).
- Entity extraction patterns are compiled once at startup via `once_cell::Lazy`.
