//! Audio transcription providers for multimodal AI capabilities.
//!
//! Supports transcription via the OpenAI Whisper API.
//!
//! # Usage
//!
//! ```no_run
//! use std::path::Path;
//! use engram::multimodal::audio::{AudioTranscriberFactory, AudioTranscriber};
//!
//! # async fn run() -> engram::error::Result<()> {
//! let transcriber = AudioTranscriberFactory::from_env()?;
//! let result = transcriber.transcribe(Path::new("audio.mp3")).await?;
//! println!("Transcription: {}", result.text);
//! # Ok(())
//! # }
//! ```

use std::path::Path;

use async_trait::async_trait;

use crate::error::{EngramError, Result};

// ── Data Structures ───────────────────────────────────────────────────────────

/// A time-stamped segment of transcribed audio
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionSegment {
    /// Start time of the segment in seconds
    pub start_secs: f64,
    /// End time of the segment in seconds
    pub end_secs: f64,
    /// Transcribed text for this segment
    pub text: String,
}

/// The result of audio transcription
#[derive(Debug, Clone)]
pub struct Transcription {
    /// Full transcribed text
    pub text: String,
    /// Detected or specified language (e.g. "en", "pt")
    pub language: Option<String>,
    /// Total duration of the audio in seconds
    pub duration_secs: f64,
    /// Time-stamped segments (may be empty if the provider does not return segments)
    pub segments: Vec<TranscriptionSegment>,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

/// Trait for audio transcription providers
#[async_trait]
pub trait AudioTranscriber: Send + Sync {
    /// Transcribe an audio file at the given path.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, the format is unsupported,
    /// or the transcription API call fails.
    async fn transcribe(&self, audio_path: &Path) -> Result<Transcription>;

    /// Returns the list of supported audio format extensions (e.g. "mp3", "wav").
    fn supported_formats(&self) -> Vec<&str>;
}

// ── OpenAI Whisper Implementation ─────────────────────────────────────────────

/// Transcription provider backed by the OpenAI Whisper API.
///
/// Calls `POST https://api.openai.com/v1/audio/transcriptions` with
/// `verbose_json` response format to retrieve per-segment timing.
pub struct WhisperTranscriber {
    api_key: String,
    client: reqwest::Client,
    /// Whisper model to use (default: `whisper-1`)
    model: String,
}

impl WhisperTranscriber {
    /// Create a new `WhisperTranscriber` with the default model (`whisper-1`).
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            model: "whisper-1".to_string(),
        }
    }

    /// Create a new `WhisperTranscriber` with a specific model name.
    pub fn with_model(api_key: String, model: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
            model,
        }
    }

    /// Build a multipart form body for the Whisper API.
    fn build_form(
        &self,
        file_bytes: Vec<u8>,
        filename: String,
        mime_type: &str,
    ) -> reqwest::multipart::Form {
        let file_part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(filename)
            .mime_str(mime_type)
            .unwrap_or_else(|_| reqwest::multipart::Part::bytes(vec![]));

        reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", "verbose_json")
    }

    /// Infer the MIME type from the file extension.
    fn mime_for_ext(ext: &str) -> &'static str {
        match ext {
            "mp3" => "audio/mpeg",
            "mp4" => "audio/mp4",
            "wav" => "audio/wav",
            "m4a" => "audio/mp4",
            "webm" => "audio/webm",
            _ => "application/octet-stream",
        }
    }

    /// Parse the Whisper verbose_json response into a [`Transcription`].
    fn parse_response(data: &serde_json::Value) -> Result<Transcription> {
        let text = data["text"]
            .as_str()
            .ok_or_else(|| {
                EngramError::Internal("Invalid Whisper response: missing 'text' field".to_string())
            })?
            .to_string();

        let language = data["language"].as_str().map(|s| s.to_string());

        let duration_secs = data["duration"].as_f64().unwrap_or(0.0);

        let segments = data["segments"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|seg| {
                        let start = seg["start"].as_f64()?;
                        let end = seg["end"].as_f64()?;
                        let seg_text = seg["text"].as_str()?.to_string();
                        Some(TranscriptionSegment {
                            start_secs: start,
                            end_secs: end,
                            text: seg_text,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(Transcription {
            text,
            language,
            duration_secs,
            segments,
        })
    }
}

#[async_trait]
impl AudioTranscriber for WhisperTranscriber {
    async fn transcribe(&self, audio_path: &Path) -> Result<Transcription> {
        // Validate extension
        let ext = audio_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !self.supported_formats().contains(&ext.as_str()) {
            return Err(EngramError::InvalidInput(format!(
                "Unsupported audio format: '{}'. Supported: {}",
                ext,
                self.supported_formats().join(", ")
            )));
        }

        let file_bytes = std::fs::read(audio_path).map_err(EngramError::Io)?;
        let filename = audio_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio")
            .to_string();
        let mime_type = Self::mime_for_ext(&ext);

        let form = self.build_form(file_bytes, filename, mime_type);

        let response = self
            .client
            .post("https://api.openai.com/v1/audio/transcriptions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .map_err(EngramError::Http)?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(EngramError::Internal(format!(
                "Whisper API error {}: {}",
                status, body
            )));
        }

        let data: serde_json::Value = response.json().await.map_err(EngramError::Http)?;
        Self::parse_response(&data)
    }

    fn supported_formats(&self) -> Vec<&str> {
        vec!["mp3", "mp4", "wav", "m4a", "webm"]
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Factory that selects an audio transcription provider based on environment variables.
///
/// Priority:
/// 1. `OPENAI_API_KEY` — uses OpenAI Whisper API
/// 2. Neither set — returns [`EngramError::Config`]
pub struct AudioTranscriberFactory;

impl AudioTranscriberFactory {
    /// Create a transcription provider from environment variables.
    ///
    /// # Errors
    ///
    /// Returns `EngramError::Config` if `OPENAI_API_KEY` is not set.
    pub fn from_env() -> Result<Box<dyn AudioTranscriber>> {
        if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            Ok(Box::new(WhisperTranscriber::new(key)))
        } else {
            Err(EngramError::Config(
                "No audio transcription provider API key found. Set OPENAI_API_KEY".to_string(),
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

    /// RAII guard: saves and restores env vars, holds the env mutex.
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

    fn clear_audio_env() -> EnvGuard {
        let lock = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        EnvGuard::save_and_clear(lock, &["OPENAI_API_KEY"])
    }

    // ── Factory tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_factory_returns_transcriber_when_openai_key_set() {
        let _guard = clear_audio_env();
        std::env::set_var("OPENAI_API_KEY", "test-key");

        let result = AudioTranscriberFactory::from_env();
        assert!(result.is_ok(), "should succeed when OPENAI_API_KEY is set");
    }

    #[test]
    fn test_factory_returns_config_error_when_no_key() {
        let _guard = clear_audio_env();

        let result = AudioTranscriberFactory::from_env();
        assert!(result.is_err(), "should fail when no API key is set");
        match result.err().unwrap() {
            EngramError::Config(msg) => {
                assert!(
                    msg.contains("OPENAI_API_KEY"),
                    "error should mention OPENAI_API_KEY, got: {msg}"
                );
            }
            other => panic!("expected Config error, got: {other:?}"),
        }
    }

    // ── supported_formats tests ───────────────────────────────────────────────

    #[test]
    fn test_whisper_supported_formats() {
        let transcriber = WhisperTranscriber::new("key".to_string());
        let formats = transcriber.supported_formats();

        assert!(formats.contains(&"mp3"), "mp3 should be supported");
        assert!(formats.contains(&"mp4"), "mp4 should be supported");
        assert!(formats.contains(&"wav"), "wav should be supported");
        assert!(formats.contains(&"m4a"), "m4a should be supported");
        assert!(formats.contains(&"webm"), "webm should be supported");
        assert!(!formats.contains(&"flac"), "flac should not be listed");
    }

    // ── parse_response tests (mock HTTP responses) ────────────────────────────

    #[test]
    fn test_parse_response_full_verbose_json() {
        let json = serde_json::json!({
            "text": "Hello world",
            "language": "en",
            "duration": 3.5,
            "segments": [
                {"start": 0.0, "end": 1.2, "text": "Hello"},
                {"start": 1.2, "end": 3.5, "text": " world"}
            ]
        });

        let result =
            WhisperTranscriber::parse_response(&json).expect("should parse valid verbose_json");

        assert_eq!(result.text, "Hello world");
        assert_eq!(result.language, Some("en".to_string()));
        assert!((result.duration_secs - 3.5).abs() < 1e-9);
        assert_eq!(result.segments.len(), 2);
        assert_eq!(result.segments[0].start_secs, 0.0);
        assert_eq!(result.segments[0].end_secs, 1.2);
        assert_eq!(result.segments[0].text, "Hello");
        assert_eq!(result.segments[1].text, " world");
    }

    #[test]
    fn test_parse_response_no_segments() {
        let json = serde_json::json!({
            "text": "Simple transcription",
            "language": "pt",
            "duration": 5.0,
            "segments": []
        });

        let result = WhisperTranscriber::parse_response(&json)
            .expect("should parse response with empty segments");

        assert_eq!(result.text, "Simple transcription");
        assert_eq!(result.language, Some("pt".to_string()));
        assert_eq!(result.segments.len(), 0);
    }

    #[test]
    fn test_parse_response_missing_language_field() {
        let json = serde_json::json!({
            "text": "No language field",
            "duration": 2.0
        });

        let result = WhisperTranscriber::parse_response(&json)
            .expect("should parse response without language field");

        assert_eq!(result.text, "No language field");
        assert!(
            result.language.is_none(),
            "language should be None when missing"
        );
        assert_eq!(result.segments.len(), 0);
    }

    #[test]
    fn test_parse_response_missing_text_returns_error() {
        let json = serde_json::json!({
            "language": "en",
            "duration": 1.0
        });

        let result = WhisperTranscriber::parse_response(&json);
        assert!(
            result.is_err(),
            "should return error when 'text' field is missing"
        );
        match result.err().unwrap() {
            EngramError::Internal(msg) => {
                assert!(
                    msg.contains("text"),
                    "error should mention 'text' field, got: {msg}"
                );
            }
            other => panic!("expected Internal error, got: {other:?}"),
        }
    }

    #[test]
    fn test_parse_response_skips_malformed_segments() {
        let json = serde_json::json!({
            "text": "Partial segments",
            "duration": 4.0,
            "segments": [
                {"start": 0.0, "end": 1.0, "text": "Good segment"},
                {"start": 1.0},                          // missing "end" and "text"
                {"end": 3.0, "text": "Missing start"}     // missing "start"
            ]
        });

        let result = WhisperTranscriber::parse_response(&json)
            .expect("should parse response, skipping malformed segments");

        // Only the first segment is valid
        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.segments[0].text, "Good segment");
    }

    // ── unsupported format test ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_transcribe_unsupported_format_returns_error() {
        let transcriber = WhisperTranscriber::new("test-key".to_string());
        // Use a tempfile with unsupported extension — no HTTP call will be made
        let tmp =
            tempfile::NamedTempFile::with_suffix(".flac").expect("failed to create temp file");
        let result = transcriber.transcribe(tmp.path()).await;

        assert!(result.is_err(), "should reject unsupported format");
        match result.err().unwrap() {
            EngramError::InvalidInput(msg) => {
                assert!(
                    msg.contains("flac"),
                    "error should mention rejected format, got: {msg}"
                );
            }
            other => panic!("expected InvalidInput error, got: {other:?}"),
        }
    }

    // ── mime_for_ext tests ────────────────────────────────────────────────────

    #[test]
    fn test_mime_for_ext_known_formats() {
        assert_eq!(WhisperTranscriber::mime_for_ext("mp3"), "audio/mpeg");
        assert_eq!(WhisperTranscriber::mime_for_ext("mp4"), "audio/mp4");
        assert_eq!(WhisperTranscriber::mime_for_ext("wav"), "audio/wav");
        assert_eq!(WhisperTranscriber::mime_for_ext("m4a"), "audio/mp4");
        assert_eq!(WhisperTranscriber::mime_for_ext("webm"), "audio/webm");
    }

    #[test]
    fn test_mime_for_ext_unknown_falls_back_to_octet_stream() {
        assert_eq!(
            WhisperTranscriber::mime_for_ext("xyz"),
            "application/octet-stream"
        );
    }
}
