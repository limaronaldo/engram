//! CLIP-style multimodal embedding provider.
//!
//! Maps both text and images into a shared vector space, enabling cross-modal
//! similarity search (e.g. "find memories similar to this image").
//!
//! # Implementation Strategy
//!
//! Uses a **description-mediated** approach:
//! 1. For text: delegates to `OpenAIEmbedder` with `text-embedding-3-small`.
//! 2. For images: uses a `VisionProvider` to generate a textual description of
//!    the image, then embeds that description with the same text embedder.
//!
//! This pragmatic approach produces vectors in the same space for both modalities
//! without requiring a custom CLIP ONNX model. True CLIP vectors can be added
//! later via the `onnx-embed` feature.
//!
//! # Feature Gate
//!
//! This module is compiled only when the `multimodal` feature is active.
//!
//! # Usage
//!
//! ```no_run
//! # #[cfg(feature = "multimodal")]
//! # async fn run() -> engram::error::Result<()> {
//! use engram::embedding::clip::{ClipEmbedder, MultimodalEmbedder};
//!
//! let embedder = ClipEmbedder::from_env()?;
//!
//! // Embed text
//! let text_vec = embedder.embed("a dashboard screenshot")?;
//!
//! // Embed image
//! let image_bytes = std::fs::read("/tmp/dashboard.png").unwrap();
//! let image_vec = embedder.embed_image_sync(&image_bytes, "image/png")?;
//!
//! // Both vectors are in the same 1536-dimensional space
//! assert_eq!(text_vec.len(), image_vec.len());
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;

use crate::error::{EngramError, Result};
use crate::multimodal::vision::{VisionInput, VisionOptions, VisionProviderFactory};

use super::{Embedder, OpenAIEmbedder};

// ── MultimodalEmbedder trait ─────────────────────────────────────────────────

/// Extension of [`Embedder`] with cross-modal capabilities.
///
/// Implementors can embed both text and raw image bytes into a shared vector
/// space, enabling cross-modal similarity queries.
pub trait MultimodalEmbedder: Embedder {
    /// Generate an embedding vector for a raw image.
    ///
    /// The returned vector MUST have the same dimensionality as the text
    /// embeddings produced by [`Embedder::embed`].
    ///
    /// # Arguments
    /// * `image_bytes` — raw bytes of the image
    /// * `mime_type`   — MIME type (e.g. `"image/png"`, `"image/jpeg"`)
    fn embed_image_sync(&self, image_bytes: &[u8], mime_type: &str) -> Result<Vec<f32>>;

    /// Returns the provider name, e.g. `"clip"`.
    fn multimodal_provider_name(&self) -> &str;
}

// ── ClipEmbedder ─────────────────────────────────────────────────────────────

/// Description-mediated CLIP-style embedder.
///
/// Text → OpenAI `text-embedding-3-small` embeddings.
/// Image → Vision model description → same text embedder.
pub struct ClipEmbedder {
    text_embedder: Arc<OpenAIEmbedder>,
}

impl ClipEmbedder {
    /// Create a new `ClipEmbedder` with an explicit API key.
    ///
    /// Both the text embedder and the vision provider will use the same key.
    pub fn new(api_key: String) -> Self {
        Self {
            text_embedder: Arc::new(OpenAIEmbedder::new(api_key)),
        }
    }

    /// Create a `ClipEmbedder` from the `OPENAI_API_KEY` environment variable.
    ///
    /// # Errors
    ///
    /// Returns `EngramError::Config` if `OPENAI_API_KEY` is not set.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
            EngramError::Config(
                "OPENAI_API_KEY is required for CLIP embeddings".to_string(),
            )
        })?;
        Ok(Self::new(api_key))
    }

    /// Async version: generate an image embedding by describing the image first.
    pub async fn embed_image_async(
        &self,
        image_bytes: &[u8],
        mime_type: &str,
    ) -> Result<Vec<f32>> {
        // Step 1: get a vision provider from the environment
        let vision = VisionProviderFactory::from_env().map_err(|e| {
            EngramError::Config(format!(
                "Vision provider required for image embedding: {}",
                e
            ))
        })?;

        // Step 2: generate a detailed textual description of the image
        let input = VisionInput {
            image_bytes: image_bytes.to_vec(),
            mime_type: mime_type.to_string(),
        };
        let opts = VisionOptions {
            prompt: Some(
                "Describe this image in detail, including objects, colors, layout, and any text visible. Be precise and comprehensive.".to_string(),
            ),
            max_tokens: Some(512),
        };
        let description = vision.describe_image(input, opts).await?;

        // Step 3: embed the description text
        self.text_embedder.embed_async(&description.text).await
    }
}

// ── Embedder impl ─────────────────────────────────────────────────────────────

impl Embedder for ClipEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.text_embedder.embed(text)
    }

    fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        self.text_embedder.embed_batch(texts)
    }

    fn dimensions(&self) -> usize {
        self.text_embedder.dimensions()
    }

    fn model_name(&self) -> &str {
        "clip-description-mediated"
    }
}

// ── MultimodalEmbedder impl ──────────────────────────────────────────────────

impl MultimodalEmbedder for ClipEmbedder {
    fn embed_image_sync(&self, image_bytes: &[u8], mime_type: &str) -> Result<Vec<f32>> {
        // Blocking wrapper around the async implementation
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(self.embed_image_async(image_bytes, mime_type))
        })
    }

    fn multimodal_provider_name(&self) -> &str {
        "clip"
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Create a `ClipEmbedder` from environment variables.
///
/// Requires `OPENAI_API_KEY` to be set. The vision provider (`GEMINI_API_KEY`
/// or `OPENAI_API_KEY`) is resolved lazily at embedding time.
pub fn create_clip_embedder() -> Result<Arc<ClipEmbedder>> {
    ClipEmbedder::from_env().map(Arc::new)
}

// ── EmbeddingRegistry integration ────────────────────────────────────────────

/// Register the CLIP embedder in a provider name string.
///
/// When `ENGRAM_EMBEDDING_MODEL=clip`, the registry should pick this provider.
pub const CLIP_PROVIDER_NAME: &str = "clip";

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: verify `ClipEmbedder` implements both `Embedder` and
    /// `MultimodalEmbedder` at the type level.
    #[test]
    fn test_clip_embedder_implements_multimodal_embedder() {
        // Compile-time check — if this compiles, the traits are implemented
        fn assert_multimodal<T: MultimodalEmbedder + Embedder>() {}
        assert_multimodal::<ClipEmbedder>();
    }

    #[test]
    fn test_clip_provider_name() {
        // Build a ClipEmbedder with a dummy key (no API calls in this test)
        let embedder = ClipEmbedder::new("dummy-key".to_string());
        assert_eq!(embedder.multimodal_provider_name(), "clip");
        assert_eq!(embedder.model_name(), "clip-description-mediated");
    }

    #[test]
    fn test_clip_dimensions_match_openai() {
        let embedder = ClipEmbedder::new("dummy-key".to_string());
        // text-embedding-3-small produces 1536-dimensional vectors
        assert_eq!(embedder.dimensions(), 1536);
    }

    #[test]
    fn test_from_env_fails_without_api_key() {
        // Save and clear the key
        let saved = std::env::var("OPENAI_API_KEY").ok();
        std::env::remove_var("OPENAI_API_KEY");

        let result = ClipEmbedder::from_env();
        assert!(result.is_err(), "should fail without OPENAI_API_KEY");
        match result.err().unwrap() {
            EngramError::Config(msg) => {
                assert!(
                    msg.contains("OPENAI_API_KEY"),
                    "error should mention OPENAI_API_KEY"
                );
            }
            e => panic!("expected Config error, got: {:?}", e),
        }

        // Restore
        if let Some(key) = saved {
            std::env::set_var("OPENAI_API_KEY", key);
        }
    }

    #[test]
    fn test_clip_provider_constant() {
        assert_eq!(CLIP_PROVIDER_NAME, "clip");
    }

    /// Verify that `create_clip_embedder` wraps the embedder in `Arc`.
    #[test]
    fn test_create_clip_embedder_type() {
        std::env::set_var("OPENAI_API_KEY", "test-key");
        let result = create_clip_embedder();
        assert!(
            result.is_ok(),
            "create_clip_embedder should succeed when OPENAI_API_KEY is set"
        );
        // Arc<ClipEmbedder> is returned
        let arc = result.unwrap();
        assert_eq!(arc.multimodal_provider_name(), "clip");
        std::env::remove_var("OPENAI_API_KEY");
    }
}
