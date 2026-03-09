//! Cohere embedding provider
//!
//! Sends texts to the Cohere `/embed` endpoint and returns dense float vectors.
//! Default model: `embed-english-v3.0` (1024 dimensions).
//!
//! # Feature Flag
//!
//! Gated behind `#[cfg(feature = "cohere")]`. Requires the `cohere` feature to be
//! enabled at build time.
//!
//! # Example
//!
//! ```rust,no_run
//! # #[cfg(feature = "cohere")]
//! # {
//! use engram::embedding::cohere::{CohereConfig, CohereEmbedder};
//! use engram::embedding::Embedder;
//!
//! let config = CohereConfig {
//!     api_key: "co-...".to_string(),
//!     ..CohereConfig::default()
//! };
//! let embedder = CohereEmbedder::new(config);
//! let embedding = embedder.embed("Hello, world!").unwrap();
//! assert_eq!(embedding.len(), 1024);
//! # }
//! ```

#[cfg(feature = "cohere")]
mod inner {
    use crate::embedding::Embedder;
    use crate::error::{EngramError, Result};

    /// Configuration for the Cohere embedding provider.
    #[derive(Debug, Clone)]
    pub struct CohereConfig {
        /// Cohere API key (required).
        pub api_key: String,
        /// Model name (e.g. `embed-english-v3.0`).
        pub model: String,
        /// Base URL of the Cohere API.
        pub base_url: String,
        /// Expected number of dimensions in the output embedding vector.
        pub dimensions: usize,
    }

    impl Default for CohereConfig {
        fn default() -> Self {
            Self {
                api_key: String::new(),
                model: "embed-english-v3.0".to_string(),
                base_url: "https://api.cohere.ai/v1".to_string(),
                dimensions: 1024,
            }
        }
    }

    /// Embedding client backed by the Cohere API.
    pub struct CohereEmbedder {
        config: CohereConfig,
        client: reqwest::Client,
    }

    impl CohereEmbedder {
        /// Create a new embedder with the given configuration.
        pub fn new(config: CohereConfig) -> Self {
            Self {
                config,
                client: reqwest::Client::new(),
            }
        }

        /// Async call to the Cohere `/embed` endpoint.
        ///
        /// The `input_type` is fixed to `"search_document"` which is suitable
        /// for indexing content into a memory store.
        pub async fn embed_async(&self, text: &str) -> Result<Vec<f32>> {
            let url = format!("{}/embed", self.config.base_url);

            let response = self
                .client
                .post(&url)
                .header(
                    "Authorization",
                    format!("Bearer {}", self.config.api_key),
                )
                .json(&serde_json::json!({
                    "texts": [text],
                    "model": self.config.model,
                    "input_type": "search_document",
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(EngramError::Embedding(format!(
                    "Cohere API error {status}: {body}"
                )));
            }

            let data: serde_json::Value = response.json().await?;
            let embeddings = data["embeddings"]
                .as_array()
                .ok_or_else(|| {
                    EngramError::Embedding(
                        "Cohere response missing 'embeddings' field".to_string(),
                    )
                })?;

            let embedding: Vec<f32> = embeddings
                .first()
                .and_then(|e| e.as_array())
                .ok_or_else(|| {
                    EngramError::Embedding(
                        "Cohere response 'embeddings[0]' is missing or not an array".to_string(),
                    )
                })?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            if embedding.is_empty() {
                return Err(EngramError::Embedding(
                    "Cohere returned an empty embedding vector".to_string(),
                ));
            }

            Ok(embedding)
        }

        /// Async batch call to the Cohere `/embed` endpoint.
        ///
        /// Cohere natively supports batching; all texts are sent in a single request.
        pub async fn embed_batch_async(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            if texts.is_empty() {
                return Ok(vec![]);
            }

            let url = format!("{}/embed", self.config.base_url);

            let response = self
                .client
                .post(&url)
                .header(
                    "Authorization",
                    format!("Bearer {}", self.config.api_key),
                )
                .json(&serde_json::json!({
                    "texts": texts,
                    "model": self.config.model,
                    "input_type": "search_document",
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(EngramError::Embedding(format!(
                    "Cohere API error {status}: {body}"
                )));
            }

            let data: serde_json::Value = response.json().await?;
            let raw = data["embeddings"]
                .as_array()
                .ok_or_else(|| {
                    EngramError::Embedding(
                        "Cohere response missing 'embeddings' field".to_string(),
                    )
                })?;

            let embeddings: Vec<Vec<f32>> = raw
                .iter()
                .map(|e| {
                    e.as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_f64().map(|f| f as f32))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect();

            Ok(embeddings)
        }
    }

    impl Embedder for CohereEmbedder {
        fn embed(&self, text: &str) -> crate::error::Result<Vec<f32>> {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(self.embed_async(text))
            })
        }

        fn embed_batch(&self, texts: &[&str]) -> crate::error::Result<Vec<Vec<f32>>> {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(self.embed_batch_async(texts))
            })
        }

        fn dimensions(&self) -> usize {
            self.config.dimensions
        }

        fn model_name(&self) -> &str {
            &self.config.model
        }
    }
}

#[cfg(feature = "cohere")]
pub use inner::{CohereConfig, CohereEmbedder};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// A lightweight stub that mimics CohereEmbedder behaviour without HTTP.
    struct StubCohereEmbedder {
        dimensions: usize,
        model: String,
    }

    impl StubCohereEmbedder {
        fn new(dimensions: usize) -> Self {
            Self {
                dimensions,
                model: "embed-english-v3.0".to_string(),
            }
        }

        /// Deterministic embedding: hash bytes into a fixed-length float vector.
        fn embed_stub(&self, text: &str) -> Vec<f32> {
            let mut embedding = vec![0.0_f32; self.dimensions];
            for (i, byte) in text.bytes().enumerate() {
                // Spread bytes across dimensions using two different offsets to
                // reduce collision when texts differ only by a few characters.
                embedding[i % self.dimensions] += byte as f32;
                embedding[(i * 7 + 13) % self.dimensions] -= byte as f32 * 0.1;
            }
            // L2 normalise
            let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in &mut embedding {
                    *x /= norm;
                }
            }
            embedding
        }

        fn embed_batch_stub(&self, texts: &[&str]) -> Vec<Vec<f32>> {
            texts.iter().map(|t| self.embed_stub(t)).collect()
        }
    }

    #[test]
    fn test_stub_embed_returns_correct_dimensions() {
        let embedder = StubCohereEmbedder::new(1024);
        let result = embedder.embed_stub("hello world");
        assert_eq!(result.len(), 1024, "embedding must have 1024 dimensions");
    }

    #[test]
    fn test_stub_embed_is_deterministic() {
        let embedder = StubCohereEmbedder::new(1024);
        let e1 = embedder.embed_stub("same text input");
        let e2 = embedder.embed_stub("same text input");
        assert_eq!(e1, e2, "same input must produce identical vectors");
    }

    #[test]
    fn test_stub_embed_different_inputs_differ() {
        let embedder = StubCohereEmbedder::new(1024);
        let e1 = embedder.embed_stub("first sentence about AI");
        let e2 = embedder.embed_stub("totally unrelated content xyz");
        assert_ne!(e1, e2, "different inputs should produce different vectors");
    }

    #[test]
    fn test_stub_embed_batch_length_matches_input() {
        let embedder = StubCohereEmbedder::new(1024);
        let texts = ["alpha", "beta", "gamma"];
        let results = embedder.embed_batch_stub(&texts);
        assert_eq!(results.len(), 3, "batch result count must match input count");
        for r in &results {
            assert_eq!(r.len(), 1024);
        }
    }

    #[test]
    fn test_stub_embed_empty_input_returns_zero_vector() {
        let embedder = StubCohereEmbedder::new(1024);
        let result = embedder.embed_stub("");
        assert_eq!(result.len(), 1024);
        assert!(
            result.iter().all(|&x| x == 0.0),
            "empty input should yield zero vector"
        );
    }

    #[cfg(feature = "cohere")]
    #[test]
    fn test_cohere_config_defaults() {
        use super::inner::CohereConfig;
        let cfg = CohereConfig::default();
        assert_eq!(cfg.model, "embed-english-v3.0");
        assert_eq!(cfg.base_url, "https://api.cohere.ai/v1");
        assert_eq!(cfg.dimensions, 1024);
        assert!(cfg.api_key.is_empty(), "default api_key must be empty");
    }

    #[cfg(feature = "cohere")]
    #[test]
    fn test_cohere_config_custom() {
        use super::inner::CohereConfig;
        let cfg = CohereConfig {
            api_key: "co-test-key".to_string(),
            model: "embed-multilingual-v3.0".to_string(),
            base_url: "https://api.cohere.ai/v1".to_string(),
            dimensions: 1024,
        };
        assert_eq!(cfg.api_key, "co-test-key");
        assert_eq!(cfg.model, "embed-multilingual-v3.0");
    }
}
