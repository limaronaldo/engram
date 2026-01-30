//! Embedding generation and async queue management (RML-873)
//!
//! Supports multiple embedding backends:
//! - OpenAI API (text-embedding-3-small) - requires `openai` feature
//! - TF-IDF fallback (no external dependencies)
//!
//! Features:
//! - LRU embedding cache with zero-copy Arc<[f32]> sharing
//! - Async queue processing for batch operations
//!
//! # Feature Flags
//!
//! - `openai`: Enables OpenAI embedding backend (requires API key)

mod cache;
mod queue;
mod tfidf;

pub use cache::{EmbeddingCache, EmbeddingCacheStats};
pub use queue::{get_embedding, get_embedding_status, EmbeddingQueue, EmbeddingWorker};
pub use tfidf::TfIdfEmbedder;

use std::sync::Arc;

use crate::error::{EngramError, Result};
use crate::types::EmbeddingConfig;

/// Trait for embedding generators
pub trait Embedder: Send + Sync {
    /// Generate embedding for a single text
    fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// Generate embeddings for multiple texts (batch)
    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }

    /// Get embedding dimensions
    fn dimensions(&self) -> usize;

    /// Get model name
    fn model_name(&self) -> &str;
}

/// OpenAI embedding client
///
/// Requires the `openai` feature to be enabled.
/// Supports OpenAI, OpenRouter, Azure OpenAI, and other OpenAI-compatible APIs.
#[cfg(feature = "openai")]
pub struct OpenAIEmbedder {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    dimensions: usize,
}

#[cfg(feature = "openai")]
impl OpenAIEmbedder {
    /// Create a new OpenAI embedder with default settings
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
            model: "text-embedding-3-small".to_string(),
            dimensions: 1536,
        }
    }

    /// Create a new OpenAI embedder with custom settings
    ///
    /// # Arguments
    /// * `api_key` - API key for authentication
    /// * `base_url` - API base URL (e.g., "https://openrouter.ai/api/v1" for OpenRouter)
    /// * `model` - Model name (e.g., "openai/text-embedding-3-small" for OpenRouter)
    /// * `dimensions` - Expected embedding dimensions (must match model output)
    pub fn with_config(
        api_key: String,
        base_url: Option<String>,
        model: Option<String>,
        dimensions: Option<usize>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            dimensions: dimensions.unwrap_or(1536),
        }
    }

    /// Legacy constructor for backwards compatibility
    pub fn with_model(api_key: String, model: String, dimensions: usize) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
            model,
            dimensions,
        }
    }

    /// Async embedding call to OpenAI-compatible API
    pub async fn embed_async(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            // OpenRouter requires HTTP-Referer header
            .header("HTTP-Referer", "https://github.com/engram")
            // Optional: helps OpenRouter track usage
            .header("X-Title", "Engram Memory")
            .json(&serde_json::json!({
                "input": text,
                "model": self.model,
            }))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(EngramError::Embedding(format!(
                "Embedding API error {}: {}",
                status, text
            )));
        }

        let data: serde_json::Value = response.json().await?;
        let embedding: Vec<f32> = data["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| EngramError::Embedding("Invalid response format".to_string()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        // Validate dimensions match configuration
        if embedding.len() != self.dimensions {
            return Err(EngramError::Embedding(format!(
                "Embedding dimensions mismatch: expected {}, got {}. Set OPENAI_EMBEDDING_DIMENSIONS={} to match your model.",
                self.dimensions, embedding.len(), embedding.len()
            )));
        }

        Ok(embedding)
    }

    /// Async batch embedding (up to 2048 inputs per call)
    pub async fn embed_batch_async(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let url = format!("{}/embeddings", self.base_url);

        // OpenAI allows up to 2048 inputs per batch
        let mut all_embeddings = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(2048) {
            let response = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                // OpenRouter requires HTTP-Referer header
                .header("HTTP-Referer", "https://github.com/engram")
                .header("X-Title", "Engram Memory")
                .json(&serde_json::json!({
                    "input": chunk,
                    "model": self.model,
                }))
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(EngramError::Embedding(format!(
                    "Embedding API error {}: {}",
                    status, text
                )));
            }

            let data: serde_json::Value = response.json().await?;
            let embeddings: Vec<Vec<f32>> = data["data"]
                .as_array()
                .ok_or_else(|| EngramError::Embedding("Invalid response format".to_string()))?
                .iter()
                .map(|item| {
                    item["embedding"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_f64().map(|f| f as f32))
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .collect();

            // Validate dimensions on first batch
            if !embeddings.is_empty() && embeddings[0].len() != self.dimensions {
                return Err(EngramError::Embedding(format!(
                    "Embedding dimensions mismatch: expected {}, got {}. Set OPENAI_EMBEDDING_DIMENSIONS={} to match your model.",
                    self.dimensions, embeddings[0].len(), embeddings[0].len()
                )));
            }

            all_embeddings.extend(embeddings);
        }

        Ok(all_embeddings)
    }
}

#[cfg(feature = "openai")]
impl Embedder for OpenAIEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // Blocking call for sync interface
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.embed_async(text))
        })
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.embed_batch_async(texts))
        })
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// Create an embedder from configuration
///
/// Available models depend on enabled features:
/// - `"tfidf"`: Always available, no external dependencies
/// - `"openai"`: Requires `openai` feature and API key
///
/// For OpenAI-compatible APIs (OpenRouter, Azure, etc.), set:
/// - `base_url`: API endpoint (e.g., "https://openrouter.ai/api/v1")
/// - `embedding_model`: Model name (e.g., "openai/text-embedding-3-small")
/// - `dimensions`: Expected output dimensions
pub fn create_embedder(config: &EmbeddingConfig) -> Result<Arc<dyn Embedder>> {
    match config.model.as_str() {
        #[cfg(feature = "openai")]
        "openai" => {
            let api_key = config
                .api_key
                .clone()
                .ok_or_else(|| EngramError::Config(
                    "OPENAI_API_KEY required when ENGRAM_EMBEDDING_MODEL=openai".to_string()
                ))?;
            Ok(Arc::new(OpenAIEmbedder::with_config(
                api_key,
                config.base_url.clone(),
                config.embedding_model.clone(),
                Some(config.dimensions),
            )))
        }
        #[cfg(not(feature = "openai"))]
        "openai" => Err(EngramError::Config(
            "OpenAI embeddings require the 'openai' feature to be enabled. Build with: cargo build --features openai".to_string(),
        )),
        "tfidf" => Ok(Arc::new(TfIdfEmbedder::new(config.dimensions))),
        _ => Err(EngramError::Config(format!(
            "Unknown embedding model: '{}'. Use 'openai' or 'tfidf'",
            config.model
        ))),
    }
}

/// Cosine similarity between two vectors
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 0.001);

        let d = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &d) + 1.0).abs() < 0.001);
    }

    #[test]
    fn test_tfidf_embedder() {
        let embedder = TfIdfEmbedder::new(384);
        let embedding = embedder.embed("Hello world").unwrap();
        assert_eq!(embedding.len(), 384);
    }
}
