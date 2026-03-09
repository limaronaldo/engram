//! Ollama embedding provider
//!
//! Connects to a local (or remote) Ollama instance to generate embeddings.
//! Default model: `nomic-embed-text` (768 dimensions).
//!
//! # Feature Flag
//!
//! Gated behind `#[cfg(feature = "ollama")]`. Requires the `ollama` feature to be
//! enabled at build time.
//!
//! # Example
//!
//! ```rust,no_run
//! # #[cfg(feature = "ollama")]
//! # {
//! use engram::embedding::ollama::{OllamaConfig, OllamaEmbedder};
//! use engram::embedding::Embedder;
//!
//! let config = OllamaConfig::default();
//! let embedder = OllamaEmbedder::new(config);
//! let embedding = embedder.embed("Hello, world!").unwrap();
//! assert_eq!(embedding.len(), 768);
//! # }
//! ```

#[cfg(feature = "ollama")]
mod inner {
    use crate::embedding::Embedder;
    use crate::error::{EngramError, Result};

    /// Configuration for the Ollama embedding provider.
    #[derive(Debug, Clone)]
    pub struct OllamaConfig {
        /// Base URL of the Ollama server (e.g. `http://localhost:11434`).
        pub base_url: String,
        /// Model to use for embeddings (e.g. `nomic-embed-text`).
        pub model: String,
        /// Expected number of dimensions in the output embedding vector.
        pub dimensions: usize,
    }

    impl Default for OllamaConfig {
        fn default() -> Self {
            Self {
                base_url: "http://localhost:11434".to_string(),
                model: "nomic-embed-text".to_string(),
                dimensions: 768,
            }
        }
    }

    /// Embedding client backed by a local Ollama instance.
    pub struct OllamaEmbedder {
        config: OllamaConfig,
        client: reqwest::Client,
    }

    impl OllamaEmbedder {
        /// Create a new embedder with the given configuration.
        pub fn new(config: OllamaConfig) -> Self {
            Self {
                config,
                client: reqwest::Client::new(),
            }
        }

        /// Async call to the Ollama `/api/embeddings` endpoint.
        pub async fn embed_async(&self, text: &str) -> Result<Vec<f32>> {
            let url = format!("{}/api/embeddings", self.config.base_url);

            let response = self
                .client
                .post(&url)
                .json(&serde_json::json!({
                    "model": self.config.model,
                    "prompt": text,
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                return Err(EngramError::Embedding(format!(
                    "Ollama API error {status}: {body}"
                )));
            }

            let data: serde_json::Value = response.json().await?;
            let embedding: Vec<f32> = data["embedding"]
                .as_array()
                .ok_or_else(|| {
                    EngramError::Embedding(
                        "Ollama response missing 'embedding' field".to_string(),
                    )
                })?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();

            if embedding.is_empty() {
                return Err(EngramError::Embedding(
                    "Ollama returned an empty embedding vector".to_string(),
                ));
            }

            Ok(embedding)
        }
    }

    impl Embedder for OllamaEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(self.embed_async(text))
            })
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            texts.iter().map(|t| self.embed(t)).collect()
        }

        fn dimensions(&self) -> usize {
            self.config.dimensions
        }

        fn model_name(&self) -> &str {
            &self.config.model
        }
    }
}

#[cfg(feature = "ollama")]
pub use inner::{OllamaConfig, OllamaEmbedder};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// A lightweight stub that mimics OllamaEmbedder behaviour without HTTP.
    struct StubOllamaEmbedder {
        dimensions: usize,
        model: String,
    }

    impl StubOllamaEmbedder {
        fn new(dimensions: usize) -> Self {
            Self {
                dimensions,
                model: "nomic-embed-text".to_string(),
            }
        }

        /// Deterministic embedding: hash bytes into a fixed-length float vector.
        fn embed_stub(&self, text: &str) -> Vec<f32> {
            let mut embedding = vec![0.0_f32; self.dimensions];
            for (i, byte) in text.bytes().enumerate() {
                embedding[i % self.dimensions] += byte as f32;
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
    }

    #[test]
    fn test_stub_embed_returns_correct_dimensions() {
        let embedder = StubOllamaEmbedder::new(768);
        let result = embedder.embed_stub("hello world");
        assert_eq!(result.len(), 768, "embedding must have 768 dimensions");
    }

    #[test]
    fn test_stub_embed_is_deterministic() {
        let embedder = StubOllamaEmbedder::new(768);
        let e1 = embedder.embed_stub("deterministic text");
        let e2 = embedder.embed_stub("deterministic text");
        assert_eq!(e1, e2, "same input must produce identical vectors");
    }

    #[test]
    fn test_stub_embed_different_inputs_differ() {
        let embedder = StubOllamaEmbedder::new(768);
        let e1 = embedder.embed_stub("first sentence");
        let e2 = embedder.embed_stub("completely different content");
        assert_ne!(e1, e2, "different inputs should produce different vectors");
    }

    #[test]
    fn test_stub_embed_empty_returns_zero_vector() {
        let embedder = StubOllamaEmbedder::new(768);
        let result = embedder.embed_stub("");
        assert_eq!(result.len(), 768);
        assert!(
            result.iter().all(|&x| x == 0.0),
            "empty input should yield zero vector"
        );
    }

    #[test]
    fn test_stub_model_name() {
        let embedder = StubOllamaEmbedder::new(768);
        assert_eq!(embedder.model, "nomic-embed-text");
    }

    #[cfg(feature = "ollama")]
    #[test]
    fn test_ollama_config_defaults() {
        use super::inner::OllamaConfig;
        let cfg = OllamaConfig::default();
        assert_eq!(cfg.base_url, "http://localhost:11434");
        assert_eq!(cfg.model, "nomic-embed-text");
        assert_eq!(cfg.dimensions, 768);
    }

    #[cfg(feature = "ollama")]
    #[test]
    fn test_ollama_config_custom() {
        use super::inner::OllamaConfig;
        let cfg = OllamaConfig {
            base_url: "http://my-server:11434".to_string(),
            model: "mxbai-embed-large".to_string(),
            dimensions: 1024,
        };
        assert_eq!(cfg.base_url, "http://my-server:11434");
        assert_eq!(cfg.model, "mxbai-embed-large");
        assert_eq!(cfg.dimensions, 1024);
    }
}
