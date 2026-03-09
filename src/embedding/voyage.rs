//! Voyage AI embedding provider
//!
//! Sends texts to the Voyage AI `/embeddings` endpoint and returns dense float
//! vectors. Default model: `voyage-2` (1024 dimensions).
//!
//! # Feature Flag
//!
//! Gated behind `#[cfg(feature = "voyage")]`. Requires the `voyage` feature to be
//! enabled at build time.
//!
//! # Example
//!
//! ```rust,no_run
//! # #[cfg(feature = "voyage")]
//! # {
//! use engram::embedding::voyage::{VoyageConfig, VoyageEmbedder};
//! use engram::embedding::Embedder;
//!
//! let config = VoyageConfig {
//!     api_key: "pa-...".to_string(),
//!     ..VoyageConfig::default()
//! };
//! let embedder = VoyageEmbedder::new(config);
//! let embedding = embedder.embed("Hello, world!").unwrap();
//! assert_eq!(embedding.len(), 1024);
//! # }
//! ```

#[cfg(feature = "voyage")]
mod inner {
    use crate::embedding::Embedder;
    use crate::error::{EngramError, Result};

    /// Configuration for the Voyage AI embedding provider.
    #[derive(Debug, Clone)]
    pub struct VoyageConfig {
        /// Voyage AI API key (required).
        pub api_key: String,
        /// Model name (e.g. `voyage-2`, `voyage-large-2`, `voyage-code-2`).
        pub model: String,
        /// Base URL of the Voyage AI API.
        pub base_url: String,
        /// Expected number of dimensions in the output embedding vector.
        pub dimensions: usize,
    }

    impl Default for VoyageConfig {
        fn default() -> Self {
            Self {
                api_key: String::new(),
                model: "voyage-2".to_string(),
                base_url: "https://api.voyageai.com/v1".to_string(),
                dimensions: 1024,
            }
        }
    }

    /// Embedding client backed by the Voyage AI API.
    pub struct VoyageEmbedder {
        config: VoyageConfig,
        client: reqwest::Client,
    }

    impl VoyageEmbedder {
        /// Create a new embedder with the given configuration.
        pub fn new(config: VoyageConfig) -> Self {
            Self {
                config,
                client: reqwest::Client::new(),
            }
        }

        /// Async call to the Voyage AI `/embeddings` endpoint for a single text.
        pub async fn embed_async(&self, text: &str) -> Result<Vec<f32>> {
            let url = format!("{}/embeddings", self.config.base_url);

            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .json(&serde_json::json!({
                    "input": [text],
                    "model": self.config.model,
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(EngramError::Embedding(format!(
                    "Voyage API error {status}: {body}"
                )));
            }

            let data: serde_json::Value = response.json().await?;

            // Response shape: {"data": [{"embedding": [f32...], "index": 0}], ...}
            let embedding: Vec<f32> = data["data"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|item| item["embedding"].as_array())
                .ok_or_else(|| {
                    EngramError::Embedding(
                        "Voyage response missing 'data[0].embedding' field".to_string(),
                    )
                })?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            if embedding.is_empty() {
                return Err(EngramError::Embedding(
                    "Voyage returned an empty embedding vector".to_string(),
                ));
            }

            Ok(embedding)
        }

        /// Async batch call to the Voyage AI `/embeddings` endpoint.
        ///
        /// Voyage AI supports batching natively — all texts are sent in a single
        /// request.
        pub async fn embed_batch_async(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            if texts.is_empty() {
                return Ok(vec![]);
            }

            let url = format!("{}/embeddings", self.config.base_url);

            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .json(&serde_json::json!({
                    "input": texts,
                    "model": self.config.model,
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(EngramError::Embedding(format!(
                    "Voyage API error {status}: {body}"
                )));
            }

            let data: serde_json::Value = response.json().await?;

            // Sort by index to maintain input order.
            let raw = data["data"].as_array().ok_or_else(|| {
                EngramError::Embedding("Voyage response missing 'data' field".to_string())
            })?;

            // Collect (index, embedding) pairs then sort by index.
            let mut indexed: Vec<(usize, Vec<f32>)> = raw
                .iter()
                .map(|item| {
                    let idx = item["index"].as_u64().unwrap_or(0) as usize;
                    let emb = item["embedding"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_f64().map(|f| f as f32))
                                .collect()
                        })
                        .unwrap_or_default();
                    (idx, emb)
                })
                .collect();

            indexed.sort_by_key(|(i, _)| *i);
            Ok(indexed.into_iter().map(|(_, emb)| emb).collect())
        }
    }

    impl Embedder for VoyageEmbedder {
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

#[cfg(feature = "voyage")]
pub use inner::{VoyageConfig, VoyageEmbedder};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// A lightweight stub that mimics VoyageEmbedder behaviour without HTTP.
    struct StubVoyageEmbedder {
        dimensions: usize,
        model: String,
    }

    impl StubVoyageEmbedder {
        fn new(dimensions: usize) -> Self {
            Self {
                dimensions,
                model: "voyage-2".to_string(),
            }
        }

        /// Deterministic embedding: accumulate byte values with positional mixing.
        fn embed_stub(&self, text: &str) -> Vec<f32> {
            let mut embedding = vec![0.0_f32; self.dimensions];
            for (i, byte) in text.bytes().enumerate() {
                let primary = i % self.dimensions;
                let secondary = (i * 31 + 17) % self.dimensions;
                embedding[primary] += byte as f32;
                embedding[secondary] += byte as f32 * 0.5;
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
        let embedder = StubVoyageEmbedder::new(1024);
        let result = embedder.embed_stub("hello world");
        assert_eq!(result.len(), 1024, "embedding must have 1024 dimensions");
    }

    #[test]
    fn test_stub_embed_is_deterministic() {
        let embedder = StubVoyageEmbedder::new(1024);
        let e1 = embedder.embed_stub("voyage determinism check");
        let e2 = embedder.embed_stub("voyage determinism check");
        assert_eq!(e1, e2, "same input must produce identical vectors");
    }

    #[test]
    fn test_stub_embed_different_inputs_differ() {
        let embedder = StubVoyageEmbedder::new(1024);
        let e1 = embedder.embed_stub("sentence about machine learning");
        let e2 = embedder.embed_stub("completely unrelated zebra topic");
        assert_ne!(e1, e2, "different inputs should produce different vectors");
    }

    #[test]
    fn test_stub_embed_batch_preserves_order() {
        let embedder = StubVoyageEmbedder::new(1024);
        let texts = ["alpha", "beta", "gamma", "delta"];
        let batch = embedder.embed_batch_stub(&texts);
        assert_eq!(batch.len(), 4, "batch count must match input count");

        // Each batch result should equal the individual result.
        for (i, text) in texts.iter().enumerate() {
            let single = embedder.embed_stub(text);
            assert_eq!(
                batch[i], single,
                "batch[{i}] must match individual embed for '{text}'"
            );
        }
    }

    #[test]
    fn test_stub_embed_empty_input_returns_zero_vector() {
        let embedder = StubVoyageEmbedder::new(1024);
        let result = embedder.embed_stub("");
        assert_eq!(result.len(), 1024);
        assert!(
            result.iter().all(|&x| x == 0.0),
            "empty input should yield zero vector"
        );
    }

    #[cfg(feature = "voyage")]
    #[test]
    fn test_voyage_config_defaults() {
        use super::inner::VoyageConfig;
        let cfg = VoyageConfig::default();
        assert_eq!(cfg.model, "voyage-2");
        assert_eq!(cfg.base_url, "https://api.voyageai.com/v1");
        assert_eq!(cfg.dimensions, 1024);
        assert!(cfg.api_key.is_empty(), "default api_key must be empty");
    }

    #[cfg(feature = "voyage")]
    #[test]
    fn test_voyage_config_custom() {
        use super::inner::VoyageConfig;
        let cfg = VoyageConfig {
            api_key: "pa-test-key".to_string(),
            model: "voyage-large-2".to_string(),
            base_url: "https://api.voyageai.com/v1".to_string(),
            dimensions: 1536,
        };
        assert_eq!(cfg.api_key, "pa-test-key");
        assert_eq!(cfg.model, "voyage-large-2");
        assert_eq!(cfg.dimensions, 1536);
    }
}
