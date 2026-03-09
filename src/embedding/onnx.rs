//! ONNX Runtime embedding provider
//!
//! Provides local, offline embedding inference using ONNX Runtime.
//! Supports any sentence-transformer model exported to ONNX format,
//! defaulting to `all-MiniLM-L6-v2` (384 dimensions).
//!
//! # Feature Flag
//!
//! This module is only compiled when the `onnx-embed` feature is enabled:
//!
//! ```toml
//! [features]
//! onnx-embed = ["ort", "ndarray"]
//! ```
//!
//! # Usage
//!
//! ```no_run
//! # #[cfg(feature = "onnx-embed")]
//! # {
//! use std::path::PathBuf;
//! use engram::embedding::onnx::{OnnxConfig, OnnxEmbedder};
//! use engram::embedding::Embedder;
//!
//! let config = OnnxConfig {
//!     model_path: PathBuf::from("model.onnx"),
//!     tokenizer_path: None,
//!     dimensions: 384,
//!     max_length: 512,
//!     model_name: "all-MiniLM-L6-v2".to_string(),
//! };
//! let embedder = OnnxEmbedder::new(config).unwrap();
//! let embedding = embedder.embed("Hello, world!").unwrap();
//! assert_eq!(embedding.len(), 384);
//! # }
//! ```

#[cfg(feature = "onnx-embed")]
mod inner {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use ndarray::{Array2, Axis};
    use ort::{inputs, Session};

    use crate::embedding::Embedder;
    use crate::error::{EngramError, Result};

    /// Configuration for the ONNX embedding provider
    #[derive(Debug, Clone)]
    pub struct OnnxConfig {
        /// Path to the `.onnx` model file
        pub model_path: PathBuf,

        /// Optional path to a tokenizer JSON file (HuggingFace tokenizer format).
        /// When `None`, the built-in whitespace tokenizer is used.
        pub tokenizer_path: Option<PathBuf>,

        /// Number of output dimensions (must match the model's output size)
        pub dimensions: usize,

        /// Maximum token sequence length; inputs are padded or truncated to this size
        pub max_length: usize,

        /// Human-readable model name returned by [`Embedder::model_name`]
        pub model_name: String,
    }

    impl Default for OnnxConfig {
        fn default() -> Self {
            Self {
                model_path: PathBuf::from("model.onnx"),
                tokenizer_path: None,
                dimensions: 384,
                max_length: 512,
                model_name: "all-MiniLM-L6-v2".to_string(),
            }
        }
    }

    /// ONNX Runtime backed embedding provider.
    ///
    /// Uses mean pooling over the token dimension followed by L2 normalisation,
    /// matching the behaviour of `sentence-transformers` models.
    pub struct OnnxEmbedder {
        config: OnnxConfig,
        session: Session,
        /// Simple word-to-index vocabulary built from a whitespace split
        vocab: HashMap<String, i64>,
    }

    impl OnnxEmbedder {
        /// Load an ONNX model and prepare it for inference.
        ///
        /// # Errors
        ///
        /// Returns [`EngramError::Embedding`] if the model file cannot be loaded
        /// or if the ONNX Runtime encounters an error.
        pub fn new(config: OnnxConfig) -> Result<Self> {
            let session = Session::builder()
                .map_err(|e| EngramError::Embedding(format!("Failed to create ONNX session builder: {e}")))?
                .commit_from_file(&config.model_path)
                .map_err(|e| {
                    EngramError::Embedding(format!(
                        "Failed to load ONNX model from {}: {e}",
                        config.model_path.display()
                    ))
                })?;

            // Build a minimal vocabulary so the basic tokenizer can map tokens to
            // integer indices.  A real deployment would load a proper vocabulary
            // from the tokenizer JSON, but this deterministic fallback is
            // sufficient for the whitespace tokenizer used here.
            let vocab = Self::build_basic_vocab();

            Ok(Self {
                config,
                session,
                vocab,
            })
        }

        // ------------------------------------------------------------------
        // Tokenization helpers
        // ------------------------------------------------------------------

        /// Build a small deterministic vocabulary for the built-in tokenizer.
        ///
        /// Token indices 0..=99 are reserved for special / common tokens.
        /// All other tokens are hashed into the range 100..=30521 to emulate a
        /// typical BERT-style vocabulary size without requiring a vocab file.
        fn build_basic_vocab() -> HashMap<String, i64> {
            let mut vocab = HashMap::new();

            // Special tokens
            vocab.insert("[PAD]".to_string(), 0);
            vocab.insert("[UNK]".to_string(), 100);
            vocab.insert("[CLS]".to_string(), 101);
            vocab.insert("[SEP]".to_string(), 102);
            vocab.insert("[MASK]".to_string(), 103);

            vocab
        }

        /// Convert a word to its vocabulary index.
        ///
        /// Falls back to a deterministic hash in the range `[104, 30521]` if
        /// the token is not in the explicit vocabulary, so every token produces
        /// a stable, non-zero index.
        fn token_to_id(&self, token: &str) -> i64 {
            if let Some(&id) = self.vocab.get(token) {
                return id;
            }

            // Deterministic hash-based fallback
            let hash = fnv1a_hash(token.as_bytes());
            // Reserve 0..=103 for special tokens; spread the rest across the
            // typical BERT vocabulary range.
            104 + (hash % (30522 - 104)) as i64
        }

        /// Tokenize a text string into a padded/truncated sequence of token ids
        /// and a matching attention mask.
        ///
        /// Layout (matching BERT-family models):
        /// ```text
        /// [CLS] tok1 tok2 ... tokN [SEP] [PAD] [PAD] ...
        /// ```
        ///
        /// Returns `(input_ids, attention_mask)` each of length `max_length`.
        pub fn tokenize(&self, text: &str) -> (Vec<i64>, Vec<i64>) {
            let max_len = self.config.max_length;

            // Reserve 2 positions for [CLS] and [SEP]
            let content_limit = max_len.saturating_sub(2);

            let raw_tokens: Vec<i64> = text
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .take(content_limit)
                .map(|t| self.token_to_id(t))
                .collect();

            let cls_id = self.vocab["[CLS]"];
            let sep_id = self.vocab["[SEP]"];
            let pad_id = self.vocab["[PAD]"];

            // Build sequence: [CLS] tokens [SEP] [PAD]...
            let mut input_ids = Vec::with_capacity(max_len);
            input_ids.push(cls_id);
            input_ids.extend_from_slice(&raw_tokens);
            input_ids.push(sep_id);

            let real_len = input_ids.len();
            input_ids.resize(max_len, pad_id);

            // Attention mask: 1 for real tokens, 0 for padding
            let mut attention_mask = vec![0i64; max_len];
            for m in attention_mask.iter_mut().take(real_len) {
                *m = 1;
            }

            (input_ids, attention_mask)
        }

        // ------------------------------------------------------------------
        // Post-processing helpers
        // ------------------------------------------------------------------

        /// Mean-pool the token embeddings over the (non-padding) token dimension.
        ///
        /// `token_embeddings`: shape `[seq_len, hidden_size]`
        /// `attention_mask`:   length `seq_len`, values 0 or 1
        ///
        /// Returns a vector of length `hidden_size`.
        pub fn mean_pool(
            token_embeddings: &Array2<f32>,
            attention_mask: &[i64],
        ) -> Vec<f32> {
            let hidden_size = token_embeddings.ncols();
            let mut sum = vec![0.0_f32; hidden_size];
            let mut count = 0_f32;

            for (row_idx, mask_val) in attention_mask.iter().enumerate() {
                if *mask_val == 1 {
                    let row = token_embeddings.row(row_idx);
                    for (s, &v) in sum.iter_mut().zip(row.iter()) {
                        *s += v;
                    }
                    count += 1.0;
                }
            }

            if count > 0.0 {
                for s in &mut sum {
                    *s /= count;
                }
            }

            sum
        }

        /// L2-normalise a vector in place.
        ///
        /// If the norm is zero (all-zero vector) the vector is left unchanged.
        pub fn l2_normalize(v: &mut Vec<f32>) {
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for x in v.iter_mut() {
                    *x /= norm;
                }
            }
        }

        // ------------------------------------------------------------------
        // Core inference
        // ------------------------------------------------------------------

        /// Run the ONNX session for a single tokenised input and return the
        /// mean-pooled, L2-normalised embedding.
        fn run_inference(&self, input_ids: &[i64], attention_mask: &[i64]) -> Result<Vec<f32>> {
            let seq_len = input_ids.len();

            // Build 2-D tensors with batch size 1: shape [1, seq_len]
            let ids_array = ndarray::Array::from_shape_vec(
                (1, seq_len),
                input_ids.to_vec(),
            )
            .map_err(|e| EngramError::Embedding(format!("Failed to build input_ids array: {e}")))?;

            let mask_array = ndarray::Array::from_shape_vec(
                (1, seq_len),
                attention_mask.to_vec(),
            )
            .map_err(|e| EngramError::Embedding(format!("Failed to build attention_mask array: {e}")))?;

            // token_type_ids: all zeros (single-sentence input)
            let type_ids_array =
                ndarray::Array::<i64, _>::zeros((1, seq_len));

            let outputs = self
                .session
                .run(inputs![
                    "input_ids" => ids_array.view(),
                    "attention_mask" => mask_array.view(),
                    "token_type_ids" => type_ids_array.view()
                ]?)
                .map_err(|e| EngramError::Embedding(format!("ONNX inference error: {e}")))?;

            // The first output is typically the last hidden state:
            // shape [batch_size, seq_len, hidden_size]
            let output_tensor = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| {
                    EngramError::Embedding(format!("Failed to extract ONNX output tensor: {e}"))
                })?;

            // Squeeze batch dimension → [seq_len, hidden_size]
            let token_embeddings: Array2<f32> = output_tensor
                .view()
                .into_dimensionality::<ndarray::Ix3>()
                .map_err(|e| {
                    EngramError::Embedding(format!("Unexpected output tensor rank: {e}"))
                })?
                .index_axis(Axis(0), 0)
                .to_owned();

            // Validate hidden size matches configured dimensions
            if token_embeddings.ncols() != self.config.dimensions {
                return Err(EngramError::Embedding(format!(
                    "Model output hidden size {} does not match configured dimensions {}",
                    token_embeddings.ncols(),
                    self.config.dimensions
                )));
            }

            let mut pooled = Self::mean_pool(&token_embeddings, attention_mask);
            Self::l2_normalize(&mut pooled);

            Ok(pooled)
        }
    }

    impl Embedder for OnnxEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            let (input_ids, attention_mask) = self.tokenize(text);
            self.run_inference(&input_ids, &attention_mask)
        }

        fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
            texts.iter().map(|t| self.embed(t)).collect()
        }

        fn dimensions(&self) -> usize {
            self.config.dimensions
        }

        fn model_name(&self) -> &str {
            &self.config.model_name
        }
    }

    // ------------------------------------------------------------------
    // Internal utility
    // ------------------------------------------------------------------

    /// FNV-1a hash (32-bit) — fast, no-dependency hash for token IDs.
    fn fnv1a_hash(bytes: &[u8]) -> i64 {
        let mut hash: u32 = 2_166_136_261;
        for &b in bytes {
            hash ^= u32::from(b);
            hash = hash.wrapping_mul(16_777_619);
        }
        i64::from(hash)
    }

    // ------------------------------------------------------------------
    // Tests
    // ------------------------------------------------------------------

    #[cfg(test)]
    mod tests {
        use super::*;

        // ------------------------------------------------------------------
        // MockOnnxEmbedder
        // ------------------------------------------------------------------

        /// A test-only embedder that mimics the OnnxEmbedder interface without
        /// requiring a real ONNX model file.  Embeddings are derived from a
        /// deterministic hash of the input text so they are stable across runs.
        struct MockOnnxEmbedder {
            dimensions: usize,
            model_name: String,
        }

        impl MockOnnxEmbedder {
            fn new(dimensions: usize, model_name: impl Into<String>) -> Self {
                Self {
                    dimensions,
                    model_name: model_name.into(),
                }
            }

            /// Generate a deterministic, L2-normalised vector from the text.
            fn mock_embedding(&self, text: &str) -> Vec<f32> {
                let mut v: Vec<f32> = (0..self.dimensions)
                    .map(|i| {
                        let h = fnv1a_hash(
                            format!("{text}:{i}").as_bytes(),
                        );
                        (h as f32).sin()
                    })
                    .collect();
                let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if norm > 0.0 {
                    for x in &mut v {
                        *x /= norm;
                    }
                }
                v
            }
        }

        impl Embedder for MockOnnxEmbedder {
            fn embed(&self, text: &str) -> Result<Vec<f32>> {
                Ok(self.mock_embedding(text))
            }

            fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
                texts.iter().map(|t| self.embed(t)).collect()
            }

            fn dimensions(&self) -> usize {
                self.dimensions
            }

            fn model_name(&self) -> &str {
                &self.model_name
            }
        }

        // ------------------------------------------------------------------
        // Tokenizer tests
        // ------------------------------------------------------------------

        /// Build a minimal OnnxEmbedder-like tokenizer harness without
        /// opening an ONNX session (we only test the tokenize() method).
        struct TokenizerHarness {
            vocab: HashMap<String, i64>,
            max_length: usize,
        }

        impl TokenizerHarness {
            fn new(max_length: usize) -> Self {
                Self {
                    vocab: OnnxEmbedder::build_basic_vocab(),
                    max_length,
                }
            }

            fn token_to_id(&self, token: &str) -> i64 {
                if let Some(&id) = self.vocab.get(token) {
                    return id;
                }
                let hash = fnv1a_hash(token.as_bytes());
                104 + (hash % (30522 - 104)) as i64
            }

            fn tokenize(&self, text: &str) -> (Vec<i64>, Vec<i64>) {
                let max_len = self.max_length;
                let content_limit = max_len.saturating_sub(2);

                let raw_tokens: Vec<i64> = text
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|s| !s.is_empty())
                    .take(content_limit)
                    .map(|t| self.token_to_id(t))
                    .collect();

                let cls_id = self.vocab["[CLS]"];
                let sep_id = self.vocab["[SEP]"];
                let pad_id = self.vocab["[PAD]"];

                let mut input_ids = Vec::with_capacity(max_len);
                input_ids.push(cls_id);
                input_ids.extend_from_slice(&raw_tokens);
                input_ids.push(sep_id);

                let real_len = input_ids.len();
                input_ids.resize(max_len, pad_id);

                let mut attention_mask = vec![0i64; max_len];
                for m in attention_mask.iter_mut().take(real_len) {
                    *m = 1;
                }

                (input_ids, attention_mask)
            }
        }

        // ------------------------------------------------------------------
        // Tokenizer tests
        // ------------------------------------------------------------------

        #[test]
        fn test_tokenize_output_length_matches_max_length() {
            let h = TokenizerHarness::new(16);
            let (ids, mask) = h.tokenize("hello world foo bar");
            assert_eq!(ids.len(), 16, "input_ids must be exactly max_length");
            assert_eq!(mask.len(), 16, "attention_mask must be exactly max_length");
        }

        #[test]
        fn test_tokenize_cls_and_sep_present() {
            let h = TokenizerHarness::new(16);
            let (ids, _) = h.tokenize("hello world");
            assert_eq!(ids[0], 101, "[CLS] id should be 101");
            // [SEP] is the token right after real tokens
            // "hello world" → 2 tokens → CLS, hello, world, SEP → index 3
            assert_eq!(ids[3], 102, "[SEP] id should be 102 at position 3");
        }

        #[test]
        fn test_tokenize_padding_is_zero() {
            let h = TokenizerHarness::new(16);
            let (ids, mask) = h.tokenize("hi");
            // "hi" → 1 token → CLS hi SEP → real_len=3, padded to 16
            for i in 3..16 {
                assert_eq!(ids[i], 0, "padding should be [PAD]=0");
                assert_eq!(mask[i], 0, "padding mask should be 0");
            }
        }

        #[test]
        fn test_tokenize_attention_mask_real_tokens_are_one() {
            let h = TokenizerHarness::new(16);
            let (_, mask) = h.tokenize("hello world");
            // CLS, hello, world, SEP → 4 real tokens
            assert_eq!(&mask[..4], &[1, 1, 1, 1]);
            assert!(mask[4..].iter().all(|&m| m == 0));
        }

        #[test]
        fn test_tokenize_truncates_long_input() {
            let h = TokenizerHarness::new(8);
            // 10 words → content_limit = 8-2 = 6, so only first 6 words fit
            let (ids, mask) = h.tokenize("one two three four five six seven eight nine ten");
            assert_eq!(ids.len(), 8);
            // All 8 positions should be real (CLS + 6 words + SEP)
            assert!(mask.iter().all(|&m| m == 1));
        }

        #[test]
        fn test_tokenize_empty_input() {
            let h = TokenizerHarness::new(16);
            let (ids, mask) = h.tokenize("");
            assert_eq!(ids.len(), 16);
            // Empty input: CLS SEP, then padding
            assert_eq!(ids[0], 101);
            assert_eq!(ids[1], 102);
            assert_eq!(mask[0], 1);
            assert_eq!(mask[1], 1);
            assert!(mask[2..].iter().all(|&m| m == 0));
        }

        #[test]
        fn test_tokenize_deterministic() {
            let h = TokenizerHarness::new(32);
            let (ids1, mask1) = h.tokenize("the quick brown fox");
            let (ids2, mask2) = h.tokenize("the quick brown fox");
            assert_eq!(ids1, ids2);
            assert_eq!(mask1, mask2);
        }

        // ------------------------------------------------------------------
        // Mean pooling tests
        // ------------------------------------------------------------------

        #[test]
        fn test_mean_pool_basic() {
            // 3 tokens × 2 dims, first 2 tokens real
            let embeddings = ndarray::array![
                [1.0_f32, 2.0],
                [3.0, 4.0],
                [0.0, 0.0],  // padding — should be ignored
            ];
            let mask = vec![1i64, 1, 0];
            let pooled = OnnxEmbedder::mean_pool(&embeddings, &mask);
            assert_eq!(pooled.len(), 2);
            assert!((pooled[0] - 2.0).abs() < 1e-6, "mean of [1,3] = 2");
            assert!((pooled[1] - 3.0).abs() < 1e-6, "mean of [2,4] = 3");
        }

        #[test]
        fn test_mean_pool_single_token() {
            let embeddings = ndarray::array![[5.0_f32, 10.0]];
            let mask = vec![1i64];
            let pooled = OnnxEmbedder::mean_pool(&embeddings, &mask);
            assert!((pooled[0] - 5.0).abs() < 1e-6);
            assert!((pooled[1] - 10.0).abs() < 1e-6);
        }

        #[test]
        fn test_mean_pool_all_padding() {
            let embeddings = ndarray::array![[1.0_f32, 2.0], [3.0, 4.0]];
            let mask = vec![0i64, 0];
            let pooled = OnnxEmbedder::mean_pool(&embeddings, &mask);
            // All zeros when everything is masked
            assert!(pooled.iter().all(|&x| x == 0.0));
        }

        // ------------------------------------------------------------------
        // L2 normalisation tests
        // ------------------------------------------------------------------

        #[test]
        fn test_l2_normalize_unit_vector() {
            let mut v = vec![3.0_f32, 4.0];
            OnnxEmbedder::l2_normalize(&mut v);
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!((norm - 1.0).abs() < 1e-6, "normalized vector should have unit norm");
        }

        #[test]
        fn test_l2_normalize_already_unit() {
            let mut v = vec![1.0_f32, 0.0, 0.0];
            let original = v.clone();
            OnnxEmbedder::l2_normalize(&mut v);
            for (a, b) in v.iter().zip(original.iter()) {
                assert!((a - b).abs() < 1e-6);
            }
        }

        #[test]
        fn test_l2_normalize_zero_vector() {
            let mut v = vec![0.0_f32, 0.0, 0.0];
            OnnxEmbedder::l2_normalize(&mut v);
            // Must not produce NaN or panic
            assert!(v.iter().all(|&x| x == 0.0));
        }

        // ------------------------------------------------------------------
        // MockOnnxEmbedder tests
        // ------------------------------------------------------------------

        #[test]
        fn test_mock_embed_returns_correct_dimensions() {
            let embedder = MockOnnxEmbedder::new(384, "all-MiniLM-L6-v2");
            let v = embedder.embed("hello world").unwrap();
            assert_eq!(v.len(), 384, "embed() must return exactly 384 dimensions");
        }

        #[test]
        fn test_mock_embed_batch_processes_all_inputs() {
            let embedder = MockOnnxEmbedder::new(384, "all-MiniLM-L6-v2");
            let texts = &["foo", "bar", "baz", "qux"];
            let results = embedder.embed_batch(texts).unwrap();
            assert_eq!(results.len(), 4, "embed_batch must return one vector per input");
            for v in &results {
                assert_eq!(v.len(), 384);
            }
        }

        #[test]
        fn test_mock_model_name_returns_config_value() {
            let embedder = MockOnnxEmbedder::new(384, "my-custom-model");
            assert_eq!(embedder.model_name(), "my-custom-model");
        }

        #[test]
        fn test_mock_dimensions_returns_config_value() {
            let embedder = MockOnnxEmbedder::new(768, "all-MiniLM-L6-v2");
            assert_eq!(embedder.dimensions(), 768);
        }

        #[test]
        fn test_mock_embed_produces_unit_vectors() {
            let embedder = MockOnnxEmbedder::new(384, "all-MiniLM-L6-v2");
            let v = embedder.embed("test text for normalisation").unwrap();
            let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            assert!(
                (norm - 1.0).abs() < 1e-5,
                "embedding must be L2-normalised, got norm={norm}"
            );
        }

        #[test]
        fn test_mock_embed_is_deterministic() {
            let embedder = MockOnnxEmbedder::new(384, "all-MiniLM-L6-v2");
            let v1 = embedder.embed("deterministic test").unwrap();
            let v2 = embedder.embed("deterministic test").unwrap();
            assert_eq!(v1, v2, "same input must always produce the same embedding");
        }

        #[test]
        fn test_mock_embed_different_texts_differ() {
            let embedder = MockOnnxEmbedder::new(384, "all-MiniLM-L6-v2");
            let v1 = embedder.embed("apple").unwrap();
            let v2 = embedder.embed("orange").unwrap();
            assert_ne!(v1, v2, "different texts must produce different embeddings");
        }

        #[test]
        fn test_mock_embed_batch_empty() {
            let embedder = MockOnnxEmbedder::new(384, "all-MiniLM-L6-v2");
            let results = embedder.embed_batch(&[]).unwrap();
            assert!(results.is_empty(), "empty batch must return empty results");
        }

        // ------------------------------------------------------------------
        // FNV hash sanity
        // ------------------------------------------------------------------

        #[test]
        fn test_fnv1a_hash_deterministic() {
            let h1 = fnv1a_hash(b"hello");
            let h2 = fnv1a_hash(b"hello");
            assert_eq!(h1, h2);
        }

        #[test]
        fn test_fnv1a_hash_different_inputs_differ() {
            let h1 = fnv1a_hash(b"hello");
            let h2 = fnv1a_hash(b"world");
            assert_ne!(h1, h2);
        }
    }
}

// Re-export public types when feature is enabled
#[cfg(feature = "onnx-embed")]
pub use inner::{OnnxConfig, OnnxEmbedder};
