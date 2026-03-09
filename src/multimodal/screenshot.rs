//! Screenshot capture and description module.
//!
//! Captures screenshots using the macOS `screencapture` CLI tool and optionally
//! describes them via a [`VisionProvider`], storing the result in the
//! `media_assets` table and as an associated memory.
//!
//! # Usage
//!
//! ```no_run
//! use engram::multimodal::screenshot::ScreenshotCapture;
//!
//! # fn main() -> engram::error::Result<()> {
//! let capture = ScreenshotCapture::new()?;
//! let result = capture.capture()?;
//! println!("Screenshot saved to {:?} ({}×{})", result.image_path, result.width, result.height);
//! # Ok(())
//! # }
//! ```

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::process::Command;

use crate::error::{EngramError, Result};
use crate::multimodal::vision::{VisionInput, VisionOptions, VisionProvider};
use crate::storage::queries::create_memory;
use crate::types::{CreateMemoryInput, MemoryTier, MemoryType};

/// Result of a screenshot capture
#[derive(Debug, Clone)]
pub struct ScreenshotResult {
    /// Path to the saved screenshot file
    pub image_path: PathBuf,
    /// Width of the screenshot in pixels
    pub width: u32,
    /// Height of the screenshot in pixels
    pub height: u32,
    /// File size in bytes
    pub file_size: u64,
    /// When the screenshot was captured
    pub timestamp: DateTime<Utc>,
    /// SHA-256 hash of the file contents (hex-encoded)
    pub file_hash: String,
}

/// Screenshot capture utility.
///
/// Uses the macOS `screencapture` CLI tool. Captured files are stored in
/// `~/.local/share/engram/screenshots/` by default.
pub struct ScreenshotCapture {
    /// Directory where screenshots are stored
    pub screenshot_dir: PathBuf,
}

impl ScreenshotCapture {
    /// Create a new `ScreenshotCapture` using the default screenshot directory.
    ///
    /// The default directory is `~/.local/share/engram/screenshots/`.
    /// It will be created if it does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn new() -> Result<Self> {
        let screenshot_dir = default_screenshot_dir()?;
        std::fs::create_dir_all(&screenshot_dir).map_err(|e| {
            EngramError::Storage(format!(
                "Failed to create screenshot directory {:?}: {}",
                screenshot_dir, e
            ))
        })?;
        Ok(Self { screenshot_dir })
    }

    /// Create a `ScreenshotCapture` that stores files in a custom directory.
    ///
    /// The directory will be created if it does not exist.
    pub fn with_dir(screenshot_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&screenshot_dir).map_err(|e| {
            EngramError::Storage(format!(
                "Failed to create screenshot directory {:?}: {}",
                screenshot_dir, e
            ))
        })?;
        Ok(Self { screenshot_dir })
    }

    /// Capture the full screen silently (no shutter sound).
    ///
    /// Equivalent to `screencapture -x <path>`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `screencapture` exits with a non-zero status
    /// - The output file cannot be read
    pub fn capture(&self) -> Result<ScreenshotResult> {
        let output_path = self.generate_path("screen");
        run_screencapture(&["-x", output_path.to_str().unwrap_or("")], &output_path)?;
        build_result(output_path)
    }

    /// Capture a specific application window by name.
    ///
    /// This method first resolves the window ID for the named application using
    /// `screencapture -l` with the Quartz window ID. If the window ID cannot be
    /// determined, the full screen is captured as a fallback.
    ///
    /// # Arguments
    ///
    /// * `app_name` — The application name as it appears in `CGWindowListCopyWindowInfo`
    ///   (e.g., `"Safari"`, `"Terminal"`)
    ///
    /// # Errors
    ///
    /// Returns an error if the screenshot cannot be captured or the file cannot be read.
    pub fn capture_window(&self, app_name: &str) -> Result<ScreenshotResult> {
        let output_path = self.generate_path(&sanitize_app_name(app_name));

        match find_window_id(app_name) {
            Some(window_id) => {
                // Use -l <window_id> to capture the specific window
                let window_id_str = window_id.to_string();
                run_screencapture(
                    &[
                        "-x",
                        "-l",
                        &window_id_str,
                        output_path.to_str().unwrap_or(""),
                    ],
                    &output_path,
                )?;
            }
            None => {
                // Fallback: capture entire screen
                tracing::warn!(
                    app = app_name,
                    "Window ID not found; falling back to full-screen capture"
                );
                run_screencapture(&["-x", output_path.to_str().unwrap_or("")], &output_path)?;
            }
        }

        build_result(output_path)
    }

    /// Generate a timestamped file path inside the screenshot directory.
    fn generate_path(&self, prefix: &str) -> PathBuf {
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S_%3f");
        self.screenshot_dir
            .join(format!("{}_{}.png", prefix, timestamp))
    }
}

// ── Describe + Store ──────────────────────────────────────────────────────────

/// Describe a screenshot with a vision provider and store the result.
///
/// This function:
/// 1. Reads the screenshot image bytes from disk.
/// 2. Calls [`VisionProvider::describe_image`] to generate a text description.
/// 3. Creates a memory with the description as content (type: `Note`, tier: `Permanent`).
/// 4. Inserts a row into the `media_assets` table linked to the memory.
/// 5. Returns the `memory_id` of the newly created memory.
///
/// # Arguments
///
/// * `screenshot` — Result from [`ScreenshotCapture::capture`] or [`ScreenshotCapture::capture_window`].
/// * `vision` — A [`VisionProvider`] implementation used to generate the description.
/// * `conn` — An open SQLite connection with the Engram schema applied.
///
/// # Errors
///
/// Returns an error if the image file cannot be read, the vision API fails,
/// or the database operations fail.
pub async fn describe_and_store(
    screenshot: &ScreenshotResult,
    vision: &dyn VisionProvider,
    conn: &Connection,
) -> Result<i64> {
    // Read image bytes
    let image_bytes = std::fs::read(&screenshot.image_path).map_err(|e| {
        EngramError::Storage(format!(
            "Failed to read screenshot {:?}: {}",
            screenshot.image_path, e
        ))
    })?;

    // Describe the image
    let vision_input = VisionInput {
        image_bytes,
        mime_type: "image/png".to_string(),
    };
    let opts = VisionOptions {
        prompt: Some(
            "Describe this screenshot in detail. Note any UI elements, text, and visible content."
                .to_string(),
        ),
        max_tokens: None,
    };
    let description = vision.describe_image(vision_input, opts).await?;

    // Create memory with the description (must come before media_assets due to FK)
    let content = format!(
        "[Screenshot] {}\n\nFile: {}\nCaptured: {}\nSize: {}×{} px ({} bytes)",
        description.text,
        screenshot.image_path.display(),
        screenshot.timestamp.to_rfc3339(),
        screenshot.width,
        screenshot.height,
        screenshot.file_size,
    );

    let memory_input = CreateMemoryInput {
        content,
        memory_type: MemoryType::Note,
        tags: vec!["screenshot".to_string(), "multimodal".to_string()],
        tier: MemoryTier::Permanent,
        ..Default::default()
    };

    let memory = create_memory(conn, &memory_input)?;

    // Insert into media_assets, linked to the newly-created memory
    insert_media_asset(
        conn,
        memory.id,
        screenshot,
        &description.text,
        &description.provider,
        &description.model,
    )?;

    Ok(memory.id)
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Returns the default directory for screenshots: `~/.local/share/engram/screenshots/`.
fn default_screenshot_dir() -> Result<PathBuf> {
    let base = dirs::data_local_dir()
        .ok_or_else(|| EngramError::Config("Cannot determine local data directory".to_string()))?;
    Ok(base.join("engram").join("screenshots"))
}

/// Invoke `screencapture` with the given arguments.
///
/// The function waits for the process to exit and returns an error if
/// the exit status is non-zero or the expected output file is absent.
fn run_screencapture(args: &[&str], expected_output: &PathBuf) -> Result<()> {
    let status = Command::new("screencapture")
        .args(args)
        .status()
        .map_err(|e| EngramError::Storage(format!("Failed to launch screencapture: {}", e)))?;

    if !status.success() {
        return Err(EngramError::Storage(format!(
            "screencapture exited with status {:?}",
            status.code()
        )));
    }

    if !expected_output.exists() {
        return Err(EngramError::Storage(format!(
            "screencapture did not produce output file: {:?}",
            expected_output
        )));
    }

    Ok(())
}

/// Build a [`ScreenshotResult`] from the given file path.
fn build_result(image_path: PathBuf) -> Result<ScreenshotResult> {
    let metadata = std::fs::metadata(&image_path).map_err(|e| {
        EngramError::Storage(format!(
            "Cannot read screenshot metadata {:?}: {}",
            image_path, e
        ))
    })?;

    let file_size = metadata.len();
    let file_data = std::fs::read(&image_path).map_err(|e| {
        EngramError::Storage(format!(
            "Cannot read screenshot file {:?}: {}",
            image_path, e
        ))
    })?;

    let file_hash = compute_sha256(&file_data);
    let (width, height) = parse_png_dimensions(&file_data);

    Ok(ScreenshotResult {
        image_path,
        width,
        height,
        file_size,
        timestamp: Utc::now(),
        file_hash,
    })
}

/// Compute SHA-256 hash of data, returning a lowercase hex string.
fn compute_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Parse PNG image dimensions from raw bytes.
///
/// PNG files store width and height as big-endian u32 values at bytes 16–19 and 20–23
/// of the file (immediately after the IHDR chunk header). Returns `(0, 0)` if the
/// data is too short or not a valid PNG.
fn parse_png_dimensions(data: &[u8]) -> (u32, u32) {
    // PNG magic: 8 bytes, then IHDR chunk: 4 length + 4 type + 4 width + 4 height
    if data.len() < 24 {
        return (0, 0);
    }

    // Check PNG signature
    let png_signature = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if &data[0..8] != png_signature {
        return (0, 0);
    }

    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    (width, height)
}

/// Sanitize an app name to a safe filename prefix (ASCII letters, digits, hyphens only).
fn sanitize_app_name(app_name: &str) -> String {
    let sanitized: String = app_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse multiple consecutive hyphens and trim leading/trailing hyphens
    sanitized
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
}

/// Try to find the Quartz window ID for a running application by name.
///
/// Uses AppleScript (`osascript`) to query `System Events` for the process ID,
/// then maps it to a window ID via the `CGWindowListCopyWindowInfo` data exposed
/// through `screencapture -L`. Returns `None` if the window ID cannot be
/// determined (e.g., the app is not running or permissions are missing).
fn find_window_id(app_name: &str) -> Option<u32> {
    // Use screencapture -L to list windows and find the one matching app_name.
    // -L outputs a JSON-ish list of windows with their IDs and owner names.
    let output = Command::new("screencapture").args(["-L"]).output().ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let app_name_lower = app_name.to_lowercase();

    // The -L output format is lines like: "<window_id> <OwnerName> ..."
    for line in stdout.lines() {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() >= 2 {
            let owner = parts[1].to_lowercase();
            if owner.contains(&app_name_lower) || app_name_lower.contains(&owner) {
                if let Ok(id) = parts[0].parse::<u32>() {
                    return Some(id);
                }
            }
        }
    }

    None
}

/// Insert a row into the `media_assets` table for the captured screenshot.
///
/// `memory_id` must refer to an existing memory row (FK constraint).
fn insert_media_asset(
    conn: &Connection,
    memory_id: i64,
    screenshot: &ScreenshotResult,
    description: &str,
    provider: &str,
    model: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO media_assets
            (memory_id, media_type, file_hash, file_path, file_size,
             mime_type, width, height, description, provider, model)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            memory_id,
            "image",
            screenshot.file_hash,
            screenshot.image_path.to_str().unwrap_or(""),
            screenshot.file_size as i64,
            "image/png",
            screenshot.width,
            screenshot.height,
            description,
            provider,
            model,
        ],
    )
    .map_err(EngramError::Database)?;

    Ok(conn.last_insert_rowid())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── Unit tests that do NOT invoke screencapture ────────────────────────────

    #[test]
    fn test_sanitize_app_name_removes_non_alphanumeric() {
        assert_eq!(sanitize_app_name("Google Chrome"), "google-chrome");
        assert_eq!(sanitize_app_name("Safari"), "safari");
        assert_eq!(sanitize_app_name("Xcode 15.0"), "xcode-15-0");
        assert_eq!(sanitize_app_name("VS Code"), "vs-code");
        assert_eq!(sanitize_app_name("---foo---"), "foo");
    }

    #[test]
    fn test_compute_sha256_is_deterministic() {
        let data = b"hello world";
        let hash1 = compute_sha256(data);
        let hash2 = compute_sha256(data);
        assert_eq!(hash1, hash2);
        // SHA-256 of "hello world" is well-known
        // Verify determinism — both calls return same hash
        assert_eq!(hash1.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
        assert_eq!(hash1.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }

    #[test]
    fn test_compute_sha256_length() {
        let data = vec![0u8; 1024];
        let hash = compute_sha256(&data);
        assert_eq!(hash.len(), 64, "SHA-256 hex string must be 64 chars");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash must be lowercase hex"
        );
    }

    #[test]
    fn test_parse_png_dimensions_valid_png() {
        // Construct a minimal PNG header
        let mut data = vec![0u8; 24];
        // PNG magic bytes
        data[0..8].copy_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        // Width = 1920 at bytes 16-19 (big-endian)
        data[16..20].copy_from_slice(&1920u32.to_be_bytes());
        // Height = 1080 at bytes 20-23 (big-endian)
        data[20..24].copy_from_slice(&1080u32.to_be_bytes());

        let (w, h) = parse_png_dimensions(&data);
        assert_eq!(w, 1920);
        assert_eq!(h, 1080);
    }

    #[test]
    fn test_parse_png_dimensions_too_short() {
        let data = vec![0u8; 10];
        let (w, h) = parse_png_dimensions(&data);
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn test_parse_png_dimensions_invalid_signature() {
        let mut data = vec![0u8; 24];
        // Wrong magic bytes (JPEG header instead)
        data[0..4].copy_from_slice(&[0xFF, 0xD8, 0xFF, 0xE0]);
        data[16..20].copy_from_slice(&1920u32.to_be_bytes());
        data[20..24].copy_from_slice(&1080u32.to_be_bytes());

        let (w, h) = parse_png_dimensions(&data);
        assert_eq!(w, 0);
        assert_eq!(h, 0);
    }

    #[test]
    fn test_screenshot_capture_creates_directory() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("a").join("b").join("screenshots");
        assert!(!nested.exists());

        let _capture = ScreenshotCapture::with_dir(nested.clone()).unwrap();
        assert!(
            nested.exists(),
            "Directory should be created by ScreenshotCapture::with_dir"
        );
    }

    #[test]
    fn test_default_screenshot_dir_is_under_engram() {
        // Verify the path structure (we can't guarantee the exact root on all machines)
        let dir = default_screenshot_dir().unwrap();
        let path_str = dir.to_string_lossy();
        assert!(
            path_str.contains("engram"),
            "Default screenshot dir should be under an 'engram' directory, got: {}",
            path_str
        );
        assert!(
            path_str.ends_with("screenshots"),
            "Default screenshot dir should end with 'screenshots', got: {}",
            path_str
        );
    }

    #[test]
    fn test_parse_png_dimensions_1x1_pixel() {
        let mut data = vec![0u8; 24];
        data[0..8].copy_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);
        data[16..20].copy_from_slice(&1u32.to_be_bytes());
        data[20..24].copy_from_slice(&1u32.to_be_bytes());

        let (w, h) = parse_png_dimensions(&data);
        assert_eq!(w, 1);
        assert_eq!(h, 1);
    }

    #[test]
    fn test_generate_path_includes_prefix_and_extension() {
        let dir = tempdir().unwrap();
        let capture = ScreenshotCapture::with_dir(dir.path().to_path_buf()).unwrap();
        let path = capture.generate_path("screen");
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(
            filename.starts_with("screen_"),
            "filename should start with 'screen_', got: {}",
            filename
        );
        assert!(
            filename.ends_with(".png"),
            "filename should end with '.png', got: {}",
            filename
        );
    }

    #[test]
    fn test_generate_path_unique_for_different_prefixes() {
        let dir = tempdir().unwrap();
        let capture = ScreenshotCapture::with_dir(dir.path().to_path_buf()).unwrap();
        let path1 = capture.generate_path("screen");
        let path2 = capture.generate_path("safari");
        assert_ne!(path1, path2);
    }
}
