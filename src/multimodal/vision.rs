//! Vision processing providers for multimodal AI capabilities.
//!
//! Supports multiple vision backends:
//! - Google Gemini (preferred, via `GEMINI_API_KEY`)
//! - OpenAI GPT-4o (fallback, via `OPENAI_API_KEY`)
//!
//! # Usage
//!
//! ```no_run
//! use engram::multimodal::vision::{VisionProviderFactory, VisionInput, VisionOptions};
//!
//! # async fn run() -> engram::error::Result<()> {
//! let provider = VisionProviderFactory::from_env()?;
//! let input = VisionInput {
//!     image_bytes: std::fs::read("image.png").unwrap(),
//!     mime_type: "image/png".to_string(),
//! };
//! let opts = VisionOptions::default();
//! let desc = provider.describe_image(input, opts).await?;
//! println!("{}", desc.text);
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use base64::Engine as _;

use crate::error::{EngramError, Result};

/// Input data for vision processing
pub struct VisionInput {
    /// Raw image bytes
    pub image_bytes: Vec<u8>,
    /// MIME type of the image (e.g., "image/png", "image/jpeg")
    pub mime_type: String,
}

/// Options to customize vision processing behavior
#[derive(Default)]
pub struct VisionOptions {
    /// Custom prompt for the vision model. Defaults to "Describe this image in detail"
    pub prompt: Option<String>,
    /// Maximum number of tokens in the response
    pub max_tokens: Option<u32>,
}

impl VisionOptions {
    /// Returns the effective prompt, using the default if none is set
    fn effective_prompt(&self) -> &str {
        self.prompt
            .as_deref()
            .unwrap_or("Describe this image in detail")
    }

    /// Returns the effective max_tokens, using the default if none is set
    fn effective_max_tokens(&self) -> u32 {
        self.max_tokens.unwrap_or(1024)
    }
}

/// The result of vision processing
pub struct ImageDescription {
    /// The generated text description of the image
    pub text: String,
    /// The model used to generate the description
    pub model: String,
    /// The provider name (e.g., "google", "openai")
    pub provider: String,
}

/// Trait for vision processing providers
#[async_trait]
pub trait VisionProvider: Send + Sync {
    /// Describe an image using the configured vision model
    async fn describe_image(
        &self,
        input: VisionInput,
        opts: VisionOptions,
    ) -> Result<ImageDescription>;

    /// Returns the provider identifier (e.g., "google", "openai")
    fn provider_name(&self) -> &str;
}

// ── Google Gemini Vision Provider ────────────────────────────────────────────

/// Vision provider backed by Google Gemini API
pub struct GeminiVisionProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl GeminiVisionProvider {
    /// Create a new Gemini vision provider using the default model (`gemini-2.0-flash`)
    pub fn new(api_key: String) -> Self {
        Self::with_model(api_key, "gemini-2.0-flash".to_string())
    }

    /// Create a new Gemini vision provider with a specific model
    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl VisionProvider for GeminiVisionProvider {
    async fn describe_image(
        &self,
        input: VisionInput,
        opts: VisionOptions,
    ) -> Result<ImageDescription> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.model, self.api_key
        );

        let image_b64 = base64::engine::general_purpose::STANDARD.encode(&input.image_bytes);

        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    {
                        "inline_data": {
                            "mime_type": input.mime_type,
                            "data": image_b64
                        }
                    },
                    {
                        "text": opts.effective_prompt()
                    }
                ]
            }],
            "generationConfig": {
                "maxOutputTokens": opts.effective_max_tokens()
            }
        });

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(EngramError::Http)?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(EngramError::Internal(format!(
                "Gemini API error {}: {}",
                status, text
            )));
        }

        let data: serde_json::Value = response.json().await.map_err(EngramError::Http)?;

        let text = data["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .ok_or_else(|| {
                EngramError::Internal("Invalid Gemini response: missing text field".to_string())
            })?
            .to_string();

        Ok(ImageDescription {
            text,
            model: self.model.clone(),
            provider: self.provider_name().to_string(),
        })
    }

    fn provider_name(&self) -> &str {
        "google"
    }
}

// ── OpenAI Vision Provider ────────────────────────────────────────────────────

/// Vision provider backed by OpenAI GPT-4o (or compatible) API
pub struct OpenAIVisionProvider {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl OpenAIVisionProvider {
    /// Create a new OpenAI vision provider using the default model (`gpt-4o`)
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "gpt-4o".to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Create a new OpenAI vision provider with a specific model
    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl VisionProvider for OpenAIVisionProvider {
    async fn describe_image(
        &self,
        input: VisionInput,
        opts: VisionOptions,
    ) -> Result<ImageDescription> {
        let image_b64 = base64::engine::general_purpose::STANDARD.encode(&input.image_bytes);
        let data_uri = format!("data:{};base64,{}", input.mime_type, image_b64);

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": opts.effective_max_tokens(),
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": data_uri
                        }
                    },
                    {
                        "type": "text",
                        "text": opts.effective_prompt()
                    }
                ]
            }]
        });

        let response = self
            .client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(EngramError::Http)?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(EngramError::Internal(format!(
                "OpenAI Vision API error {}: {}",
                status, text
            )));
        }

        let data: serde_json::Value = response.json().await.map_err(EngramError::Http)?;

        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                EngramError::Internal("Invalid OpenAI response: missing content field".to_string())
            })?
            .to_string();

        Ok(ImageDescription {
            text,
            model: self.model.clone(),
            provider: self.provider_name().to_string(),
        })
    }

    fn provider_name(&self) -> &str {
        "openai"
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Factory that selects a vision provider based on environment variables.
///
/// Priority:
/// 1. `GEMINI_API_KEY` — uses Google Gemini (default model: `gemini-2.0-flash`,
///    override with `ENGRAM_VISION_MODEL`)
/// 2. `OPENAI_API_KEY` — uses OpenAI GPT-4o
/// 3. Neither set — returns `EngramError::Config`
pub struct VisionProviderFactory;

impl VisionProviderFactory {
    /// Create a vision provider from environment variables.
    ///
    /// # Errors
    ///
    /// Returns `EngramError::Config` if neither `GEMINI_API_KEY` nor `OPENAI_API_KEY` is set.
    pub fn from_env() -> Result<Box<dyn VisionProvider>> {
        if let Ok(key) = std::env::var("GEMINI_API_KEY") {
            let model = std::env::var("ENGRAM_VISION_MODEL")
                .unwrap_or_else(|_| "gemini-2.0-flash".to_string());
            Ok(Box::new(GeminiVisionProvider::with_model(key, model)))
        } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            Ok(Box::new(OpenAIVisionProvider::new(key)))
        } else {
            Err(EngramError::Config(
                "No vision provider API key found. Set GEMINI_API_KEY or OPENAI_API_KEY"
                    .to_string(),
            ))
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// Serializes env-var tests to prevent parallel mutation races.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper to clear relevant env vars and hold the env mutex until the guard is dropped.
    fn clear_vision_env() -> EnvGuard {
        let lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        EnvGuard::save_and_clear(
            lock,
            &["GEMINI_API_KEY", "OPENAI_API_KEY", "ENGRAM_VISION_MODEL"],
        )
    }

    /// RAII guard: holds the env mutex and restores env vars on drop.
    struct EnvGuard {
        saved: Vec<(String, Option<String>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn save_and_clear(lock: std::sync::MutexGuard<'static, ()>, keys: &[&str]) -> Self {
            let saved = keys
                .iter()
                .map(|&k| (k.to_string(), std::env::var(k).ok()))
                .collect::<Vec<_>>();
            for k in keys {
                std::env::remove_var(k);
            }
            Self { saved, _lock: lock }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, v) in &self.saved {
                match v {
                    Some(val) => std::env::set_var(k, val),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    #[test]
    fn test_factory_returns_gemini_when_gemini_key_set() {
        let _guard = clear_vision_env();
        std::env::set_var("GEMINI_API_KEY", "test-gemini-key");

        let provider =
            VisionProviderFactory::from_env().expect("should succeed when GEMINI_API_KEY is set");

        assert_eq!(provider.provider_name(), "google");
    }

    #[test]
    fn test_factory_returns_openai_when_only_openai_key_set() {
        let _guard = clear_vision_env();
        std::env::set_var("OPENAI_API_KEY", "test-openai-key");

        let provider =
            VisionProviderFactory::from_env().expect("should succeed when OPENAI_API_KEY is set");

        assert_eq!(provider.provider_name(), "openai");
    }

    #[test]
    fn test_factory_returns_error_when_no_keys_set() {
        let _guard = clear_vision_env();

        let result = VisionProviderFactory::from_env();

        assert!(result.is_err(), "should fail when no API keys are set");
        let err = result.err().unwrap();
        match err {
            EngramError::Config(msg) => {
                assert!(
                    msg.contains("GEMINI_API_KEY") || msg.contains("OPENAI_API_KEY"),
                    "error message should mention expected env vars, got: {msg}"
                );
            }
            other => panic!("expected Config error, got: {other:?}"),
        }
    }

    #[test]
    fn test_factory_prefers_gemini_over_openai_when_both_set() {
        let _guard = clear_vision_env();
        std::env::set_var("GEMINI_API_KEY", "test-gemini-key");
        std::env::set_var("OPENAI_API_KEY", "test-openai-key");

        let provider = VisionProviderFactory::from_env().expect("should succeed");
        assert_eq!(
            provider.provider_name(),
            "google",
            "Gemini should take priority when both keys are present"
        );
    }

    #[test]
    fn test_vision_input_construction() {
        let input = VisionInput {
            image_bytes: vec![0xFF, 0xD8, 0xFF, 0xE0],
            mime_type: "image/jpeg".to_string(),
        };

        assert_eq!(input.image_bytes.len(), 4);
        assert_eq!(input.mime_type, "image/jpeg");
    }

    #[test]
    fn test_vision_options_defaults() {
        let opts = VisionOptions::default();

        assert!(opts.prompt.is_none());
        assert!(opts.max_tokens.is_none());
        assert_eq!(opts.effective_prompt(), "Describe this image in detail");
        assert_eq!(opts.effective_max_tokens(), 1024);
    }

    #[test]
    fn test_vision_options_custom_prompt() {
        let opts = VisionOptions {
            prompt: Some("Extract all text from this image".to_string()),
            max_tokens: Some(512),
        };

        assert_eq!(opts.effective_prompt(), "Extract all text from this image");
        assert_eq!(opts.effective_max_tokens(), 512);
    }

    #[test]
    fn test_gemini_provider_default_model() {
        let provider = GeminiVisionProvider::new("test-key".to_string());
        assert_eq!(provider.model, "gemini-2.0-flash");
        assert_eq!(provider.provider_name(), "google");
    }

    #[test]
    fn test_openai_provider_default_model() {
        let provider = OpenAIVisionProvider::new("test-key".to_string());
        assert_eq!(provider.model, "gpt-4o");
        assert_eq!(provider.provider_name(), "openai");
    }

    #[test]
    fn test_factory_respects_engram_vision_model_env() {
        let _guard = clear_vision_env();
        std::env::set_var("GEMINI_API_KEY", "test-key");
        std::env::set_var("ENGRAM_VISION_MODEL", "gemini-1.5-pro");

        // We can't inspect the internal model via the trait, but we can verify
        // the provider is created successfully with the Gemini provider name.
        let provider = VisionProviderFactory::from_env().expect("should succeed");
        assert_eq!(provider.provider_name(), "google");
    }
}
