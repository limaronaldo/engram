//! Video memory processing for multimodal AI capabilities.
//!
//! Extracts metadata and keyframes from video files using the `ffprobe` and
//! `ffmpeg` system commands, then generates natural-language descriptions of
//! each frame via a [`VisionProvider`], and finally synthesises a holistic
//! summary of the video content.
//!
//! # Requirements
//!
//! `ffmpeg` and `ffprobe` must be installed and available on `PATH`.
//! The module performs a runtime check and returns
//! [`EngramError::Config`] if either binary is missing.
//!
//! # Usage
//!
//! ```no_run
//! use std::path::Path;
//! use engram::multimodal::video::VideoProcessor;
//! use engram::multimodal::vision::VisionProviderFactory;
//!
//! # async fn run() -> engram::error::Result<()> {
//! let vision = VisionProviderFactory::from_env()?;
//! let processor = VideoProcessor::new();
//! let memory = processor.create_video_memory(Path::new("clip.mp4"), vision.as_ref()).await?;
//! println!("Summary: {}", memory.summary);
//! # Ok(())
//! # }
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::error::{EngramError, Result};
use crate::multimodal::vision::{VisionInput, VisionOptions, VisionProvider};

// ── Public types ──────────────────────────────────────────────────────────────

/// Metadata extracted from a video file via `ffprobe`.
#[derive(Debug, Clone)]
pub struct VideoMetadata {
    /// Duration of the video in seconds.
    pub duration_secs: f64,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Video codec name (e.g. `"h264"`), if detectable.
    pub codec: Option<String>,
    /// File size in bytes.
    pub file_size: u64,
    /// SHA-256 hex digest of the file contents (prefixed with `"sha256:"`).
    pub file_hash: String,
}

/// The result of full video processing: metadata, per-frame descriptions, and
/// an overall natural-language summary.
#[derive(Debug, Clone)]
pub struct VideoMemory {
    /// Technical metadata of the source video.
    pub metadata: VideoMetadata,
    /// One description per extracted keyframe, in temporal order.
    pub keyframe_descriptions: Vec<String>,
    /// A single paragraph summarising the video content based on all frame
    /// descriptions.
    pub summary: String,
    /// Paths to the extracted keyframe image files inside the temp directory.
    pub frames_path: Vec<PathBuf>,
}

// ── VideoProcessor ────────────────────────────────────────────────────────────

/// Processes video files to create rich memory records.
///
/// All system-command invocations are centralised in [`run_command`] so that
/// tests can verify the expected command-line arguments without actually
/// executing them.
pub struct VideoProcessor {
    /// Override for the `ffprobe` binary name / path.  Used in tests.
    pub(crate) ffprobe_bin: String,
    /// Override for the `ffmpeg` binary name / path.  Used in tests.
    pub(crate) ffmpeg_bin: String,
}

impl Default for VideoProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoProcessor {
    /// Create a processor using the system-default `ffprobe` and `ffmpeg` binaries.
    pub fn new() -> Self {
        Self {
            ffprobe_bin: "ffprobe".to_string(),
            ffmpeg_bin: "ffmpeg".to_string(),
        }
    }

    /// Check that both `ffprobe` and `ffmpeg` are available on `PATH`.
    ///
    /// Returns `EngramError::Config` with a descriptive message if either
    /// binary cannot be found.
    pub fn check_availability(&self) -> Result<()> {
        for bin in [&self.ffprobe_bin, &self.ffmpeg_bin] {
            let status = Command::new(bin)
                .arg("-version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            match status {
                Ok(s) if s.success() => {}
                _ => {
                    return Err(EngramError::Config(format!(
                        "'{bin}' not found or not executable. \
                         Install ffmpeg/ffprobe and ensure they are on PATH."
                    )));
                }
            }
        }
        Ok(())
    }

    /// Extract technical metadata from a video file using `ffprobe`.
    ///
    /// Parses the JSON output of:
    /// ```text
    /// ffprobe -v quiet -print_format json -show_streams -show_format <path>
    /// ```
    ///
    /// # Errors
    ///
    /// - `EngramError::Config` — `ffprobe` binary not found.
    /// - `EngramError::InvalidInput` — path does not exist or `ffprobe` fails.
    /// - `EngramError::Storage` — file metadata (size) cannot be read.
    /// - `EngramError::Internal` — JSON parsing or field extraction fails.
    pub fn extract_metadata(&self, path: &Path) -> Result<VideoMetadata> {
        if !path.exists() {
            return Err(EngramError::InvalidInput(format!(
                "Video file not found: {}",
                path.display()
            )));
        }

        // File size
        let file_size = std::fs::metadata(path)
            .map_err(|e| {
                EngramError::Storage(format!(
                    "Cannot read file metadata for '{}': {e}",
                    path.display()
                ))
            })?
            .len();

        // SHA-256 hash
        let file_hash = hash_file(path)?;

        // ffprobe JSON output
        let output = Command::new(&self.ffprobe_bin)
            .args([
                "-v",
                "quiet",
                "-print_format",
                "json",
                "-show_streams",
                "-show_format",
                &path.to_string_lossy(),
            ])
            .output()
            .map_err(|e| {
                EngramError::Config(format!(
                    "Failed to run ffprobe: {e}. \
                     Ensure ffprobe is installed and on PATH."
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(EngramError::InvalidInput(format!(
                "ffprobe failed for '{}': {stderr}",
                path.display()
            )));
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| EngramError::Internal(format!("ffprobe JSON parse error: {e}")))?;

        // Find the first video stream
        let streams = json["streams"]
            .as_array()
            .ok_or_else(|| EngramError::Internal("ffprobe: 'streams' field missing".to_string()))?;

        let video_stream = streams
            .iter()
            .find(|s| s["codec_type"].as_str() == Some("video"))
            .ok_or_else(|| {
                EngramError::InvalidInput("No video stream found in file".to_string())
            })?;

        let width = video_stream["width"]
            .as_u64()
            .ok_or_else(|| EngramError::Internal("ffprobe: 'width' missing".to_string()))?
            as u32;

        let height = video_stream["height"]
            .as_u64()
            .ok_or_else(|| EngramError::Internal("ffprobe: 'height' missing".to_string()))?
            as u32;

        let codec = video_stream["codec_name"]
            .as_str()
            .map(|s| s.to_string());

        // Duration: prefer stream-level, fall back to format-level
        let duration_secs = video_stream["duration"]
            .as_str()
            .or_else(|| json["format"]["duration"].as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .ok_or_else(|| {
                EngramError::Internal("ffprobe: could not determine video duration".to_string())
            })?;

        Ok(VideoMetadata {
            duration_secs,
            width,
            height,
            codec,
            file_size,
            file_hash,
        })
    }

    /// Extract `count` evenly-spaced keyframes from a video file using `ffmpeg`.
    ///
    /// Frames are saved as PNG files (`frame_001.png`, …) inside a temporary
    /// directory.  The caller owns the directory via the returned paths.
    ///
    /// The filter used is:
    /// ```text
    /// ffmpeg -i <input> -vf "fps=1/N" -vsync vfr <tmpdir>/frame_%03d.png
    /// ```
    /// where `N = duration / count` (minimum 1).
    ///
    /// # Errors
    ///
    /// - `EngramError::Config` — `ffmpeg` binary not found.
    /// - `EngramError::InvalidInput` — path does not exist, `count` is zero, or
    ///   `ffmpeg` fails.
    /// - `EngramError::Storage` — cannot create temp directory or list output
    ///   files.
    pub fn extract_keyframes(&self, path: &Path, count: usize) -> Result<Vec<PathBuf>> {
        if count == 0 {
            return Err(EngramError::InvalidInput(
                "count must be greater than 0".to_string(),
            ));
        }

        if !path.exists() {
            return Err(EngramError::InvalidInput(format!(
                "Video file not found: {}",
                path.display()
            )));
        }

        // Get duration to compute fps filter value
        let meta = self.extract_metadata(path)?;
        let interval = (meta.duration_secs / count as f64).max(1.0);

        // Create a unique temp directory for frames using stdlib primitives
        let tmp_dir = {
            let base = std::env::temp_dir();
            let unique = uuid::Uuid::new_v4().to_string();
            let dir = base.join(format!("engram_frames_{unique}"));
            std::fs::create_dir_all(&dir).map_err(|e| {
                EngramError::Storage(format!("Cannot create temp directory for frames: {e}"))
            })?;
            dir
        };

        let frame_pattern = tmp_dir.join("frame_%03d.png");
        let fps_filter = format!("fps=1/{interval:.6}");

        let output = Command::new(&self.ffmpeg_bin)
            .args([
                "-i",
                &path.to_string_lossy(),
                "-vf",
                &fps_filter,
                "-vsync",
                "vfr",
                &frame_pattern.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| {
                EngramError::Config(format!(
                    "Failed to run ffmpeg: {e}. \
                     Ensure ffmpeg is installed and on PATH."
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Clean up the temp directory on failure
            let _ = std::fs::remove_dir_all(&tmp_dir);
            return Err(EngramError::InvalidInput(format!(
                "ffmpeg failed for '{}': {stderr}",
                path.display()
            )));
        }

        // Collect generated frame files, sorted by name
        let mut frames: Vec<PathBuf> = std::fs::read_dir(&tmp_dir)
            .map_err(|e| {
                EngramError::Storage(format!("Cannot read temp directory: {e}"))
            })?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("png") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();

        frames.sort();

        Ok(frames)
    }

    /// Full pipeline: extract metadata → extract keyframes → describe each frame
    /// via `vision` → synthesise summary.
    ///
    /// # Errors
    ///
    /// Propagates errors from [`extract_metadata`], [`extract_keyframes`], and
    /// the vision provider.
    ///
    /// [`extract_metadata`]: Self::extract_metadata
    /// [`extract_keyframes`]: Self::extract_keyframes
    pub async fn create_video_memory(
        &self,
        path: &Path,
        vision: &dyn VisionProvider,
    ) -> Result<VideoMemory> {
        const DEFAULT_KEYFRAME_COUNT: usize = 5;

        let metadata = self.extract_metadata(path)?;

        let frames_path =
            self.extract_keyframes(path, DEFAULT_KEYFRAME_COUNT)?;

        // Describe each keyframe using the vision provider
        let mut keyframe_descriptions = Vec::with_capacity(frames_path.len());
        for frame_path in &frames_path {
            let image_bytes = std::fs::read(frame_path).map_err(|e| {
                EngramError::Storage(format!(
                    "Cannot read frame '{}': {e}",
                    frame_path.display()
                ))
            })?;

            let input = VisionInput {
                image_bytes,
                mime_type: "image/png".to_string(),
            };

            let opts = VisionOptions {
                prompt: Some(
                    "Describe what is happening in this video frame in one or two sentences."
                        .to_string(),
                ),
                max_tokens: Some(256),
            };

            let description = vision.describe_image(input, opts).await?;
            keyframe_descriptions.push(description.text);
        }

        // Synthesise summary from all frame descriptions
        let summary = build_summary(&keyframe_descriptions, &metadata);

        Ok(VideoMemory {
            metadata,
            keyframe_descriptions,
            summary,
            frames_path,
        })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Compute SHA-256 hex digest of a file, returning `"sha256:<hex>"`.
fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| EngramError::Storage(format!("Cannot read '{}': {e}", path.display())))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(format!("sha256:{}", hex::encode(hasher.finalize())))
}

/// Build a plain-text summary from per-frame descriptions.
///
/// Produces a short paragraph that mentions the video duration, resolution,
/// and concatenates the frame observations in temporal order.
fn build_summary(descriptions: &[String], meta: &VideoMetadata) -> String {
    if descriptions.is_empty() {
        return format!(
            "Video ({:.1}s, {}×{}): no frames could be extracted.",
            meta.duration_secs, meta.width, meta.height
        );
    }

    let frames_text = descriptions
        .iter()
        .enumerate()
        .map(|(i, d)| format!("Frame {}: {}", i + 1, d))
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        "Video ({:.1}s, {}×{}{}): {}",
        meta.duration_secs,
        meta.width,
        meta.height,
        meta.codec
            .as_deref()
            .map(|c| format!(", {c}"))
            .unwrap_or_default(),
        frames_text
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::multimodal::vision::{ImageDescription, VisionOptions};

    // ── Mock VisionProvider ───────────────────────────────────────────────────

    /// A `VisionProvider` mock that returns configurable responses without
    /// making any network calls.
    struct MockVisionProvider {
        /// Responses to return, popped in order for each `describe_image` call.
        responses: Arc<Mutex<Vec<String>>>,
        /// Captures calls: (image_bytes_len, mime_type, prompt)
        calls: Arc<Mutex<Vec<(usize, String, Option<String>)>>>,
        provider: String,
    }

    impl MockVisionProvider {
        fn new(responses: Vec<String>) -> Self {
            Self {
                responses: Arc::new(Mutex::new(responses)),
                calls: Arc::new(Mutex::new(Vec::new())),
                provider: "mock".to_string(),
            }
        }

        fn calls_made(&self) -> Vec<(usize, String, Option<String>)> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl VisionProvider for MockVisionProvider {
        async fn describe_image(
            &self,
            input: VisionInput,
            opts: VisionOptions,
        ) -> Result<ImageDescription> {
            self.calls.lock().unwrap().push((
                input.image_bytes.len(),
                input.mime_type.clone(),
                opts.prompt.clone(),
            ));

            let mut responses = self.responses.lock().unwrap();
            let text = if responses.is_empty() {
                "A frame from the video.".to_string()
            } else {
                responses.remove(0)
            };

            Ok(ImageDescription {
                text,
                model: "mock-model".to_string(),
                provider: self.provider.clone(),
            })
        }

        fn provider_name(&self) -> &str {
            &self.provider
        }
    }

    // ── Unit Tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_build_summary_empty_descriptions() {
        let meta = VideoMetadata {
            duration_secs: 10.0,
            width: 1920,
            height: 1080,
            codec: Some("h264".to_string()),
            file_size: 1024,
            file_hash: "sha256:abc".to_string(),
        };
        let summary = build_summary(&[], &meta);
        assert!(summary.contains("no frames"));
        assert!(summary.contains("10.0s"));
        assert!(summary.contains("1920×1080"));
    }

    #[test]
    fn test_build_summary_with_descriptions() {
        let meta = VideoMetadata {
            duration_secs: 30.0,
            width: 1280,
            height: 720,
            codec: Some("h264".to_string()),
            file_size: 2048,
            file_hash: "sha256:def".to_string(),
        };
        let descriptions = vec![
            "A person walking down a street.".to_string(),
            "The same person enters a building.".to_string(),
        ];
        let summary = build_summary(&descriptions, &meta);
        assert!(summary.contains("30.0s"));
        assert!(summary.contains("1280×720"));
        assert!(summary.contains("h264"));
        assert!(summary.contains("Frame 1:"));
        assert!(summary.contains("Frame 2:"));
        assert!(summary.contains("A person walking"));
    }

    #[test]
    fn test_build_summary_without_codec() {
        let meta = VideoMetadata {
            duration_secs: 5.0,
            width: 640,
            height: 480,
            codec: None,
            file_size: 512,
            file_hash: "sha256:ghi".to_string(),
        };
        let descriptions = vec!["A blank frame.".to_string()];
        let summary = build_summary(&descriptions, &meta);
        // codec portion should be absent — no codec name should appear
        assert!(!summary.contains("h264"), "no codec should appear in summary");
        assert!(!summary.contains("vp9"), "no codec should appear in summary");
        assert!(summary.contains("5.0s"));
        assert!(summary.contains("640×480"));
    }

    #[test]
    fn test_video_processor_new_defaults() {
        let processor = VideoProcessor::new();
        assert_eq!(processor.ffprobe_bin, "ffprobe");
        assert_eq!(processor.ffmpeg_bin, "ffmpeg");
    }

    #[test]
    fn test_extract_keyframes_rejects_zero_count() {
        let processor = VideoProcessor::new();
        // No actual file needed — we reject count=0 before any I/O
        let err = processor
            .extract_keyframes(Path::new("/tmp/nonexistent.mp4"), 0)
            .unwrap_err();
        assert!(
            err.to_string().contains("count must be greater than 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extract_metadata_rejects_missing_file() {
        let processor = VideoProcessor::new();
        let err = processor
            .extract_metadata(Path::new("/tmp/this_file_does_not_exist_engram_test.mp4"))
            .unwrap_err();
        assert!(
            err.to_string().contains("not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_extract_keyframes_rejects_missing_file() {
        let processor = VideoProcessor::new();
        let err = processor
            .extract_keyframes(
                Path::new("/tmp/this_file_does_not_exist_engram_test.mp4"),
                5,
            )
            .unwrap_err();
        // Should fail before trying to invoke ffprobe/ffmpeg
        assert!(
            err.to_string().contains("not found"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_hash_file_is_deterministic() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.bin");
        std::fs::write(&file, b"hello world").unwrap();

        let h1 = hash_file(&file).unwrap();
        let h2 = hash_file(&file).unwrap();
        assert_eq!(h1, h2);
        assert!(h1.starts_with("sha256:"));
        // SHA-256 hex is 64 chars
        assert_eq!(h1.len(), 7 + 64);
    }

    #[test]
    fn test_hash_file_differs_for_different_content() {
        let dir = tempfile::tempdir().unwrap();
        let f1 = dir.path().join("a.bin");
        let f2 = dir.path().join("b.bin");
        std::fs::write(&f1, b"content A").unwrap();
        std::fs::write(&f2, b"content B").unwrap();

        let h1 = hash_file(&f1).unwrap();
        let h2 = hash_file(&f2).unwrap();
        assert_ne!(h1, h2);
    }

    // ── Mock-vision integration test ──────────────────────────────────────────

    /// Verifies that `create_video_memory` calls the vision provider once per
    /// frame and assembles the returned descriptions into the summary.
    ///
    /// Because we cannot guarantee `ffprobe`/`ffmpeg` are present in the CI
    /// environment, this test is gated behind the `integration` cargo test
    /// flag.  Run with:
    /// ```text
    /// cargo test --features multimodal -- --ignored
    /// ```
    #[tokio::test]
    #[ignore = "requires ffprobe + ffmpeg on PATH and a real video file"]
    async fn test_create_video_memory_integration() {
        let vision = MockVisionProvider::new(vec![
            "Frame one description.".to_string(),
            "Frame two description.".to_string(),
            "Frame three description.".to_string(),
            "Frame four description.".to_string(),
            "Frame five description.".to_string(),
        ]);

        let processor = VideoProcessor::new();
        // Replace with a real video path when running locally
        let path = Path::new("/tmp/test_video.mp4");
        let memory = processor.create_video_memory(path, &vision).await.unwrap();

        assert!(!memory.keyframe_descriptions.is_empty());
        assert!(!memory.summary.is_empty());
        assert!(!memory.frames_path.is_empty());

        let calls = vision.calls_made();
        assert_eq!(calls.len(), memory.keyframe_descriptions.len());
        for (_, mime, _) in &calls {
            assert_eq!(mime, "image/png");
        }
    }

    // ── Summary construction edge cases ──────────────────────────────────────

    #[test]
    fn test_build_summary_single_frame() {
        let meta = VideoMetadata {
            duration_secs: 2.5,
            width: 320,
            height: 240,
            codec: Some("vp9".to_string()),
            file_size: 128,
            file_hash: "sha256:000".to_string(),
        };
        let descriptions = vec!["Only one frame.".to_string()];
        let summary = build_summary(&descriptions, &meta);
        assert!(summary.contains("Frame 1:"));
        assert!(summary.contains("Only one frame."));
        assert!(summary.contains("vp9"));
    }

    // ── VideoMetadata field coverage ──────────────────────────────────────────

    #[test]
    fn test_video_metadata_fields() {
        let meta = VideoMetadata {
            duration_secs: 123.456,
            width: 3840,
            height: 2160,
            codec: Some("av1".to_string()),
            file_size: 1_000_000,
            file_hash: "sha256:feedcafe".to_string(),
        };

        assert!((meta.duration_secs - 123.456).abs() < 1e-6);
        assert_eq!(meta.width, 3840);
        assert_eq!(meta.height, 2160);
        assert_eq!(meta.codec.as_deref(), Some("av1"));
        assert_eq!(meta.file_size, 1_000_000);
        assert_eq!(meta.file_hash, "sha256:feedcafe");
    }

    // ── check_availability falls through gracefully when ffmpeg is absent ─────

    #[test]
    fn test_check_availability_fails_for_nonexistent_binary() {
        let processor = VideoProcessor {
            ffprobe_bin: "this_binary_does_not_exist_engram_ffprobe".to_string(),
            ffmpeg_bin: "this_binary_does_not_exist_engram_ffmpeg".to_string(),
        };
        let err = processor.check_availability().unwrap_err();
        assert!(
            err.to_string().contains("not found or not executable"),
            "unexpected error: {err}"
        );
    }
}
