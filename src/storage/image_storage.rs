//! Image storage backend for memory images.
//!
//! This module handles uploading, retrieving, and deleting images from
//! S3-compatible storage (like Cloudflare R2). Images are stored separately
//! from the SQLite database.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

// reqwest is available when the `multimodal` or `cloud` feature is active
#[cfg(any(feature = "multimodal", feature = "cloud"))]
use reqwest;

use crate::error::{EngramError, Result};

/// Configuration for image storage
#[derive(Debug, Clone)]
pub struct ImageStorageConfig {
    /// Local storage directory for images (fallback when S3 not configured)
    pub local_dir: PathBuf,
    /// S3 bucket name (optional)
    pub s3_bucket: Option<String>,
    /// S3 endpoint URL (optional, for R2/MinIO)
    pub s3_endpoint: Option<String>,
    /// Public domain for serving images (optional)
    pub public_domain: Option<String>,
}

impl Default for ImageStorageConfig {
    fn default() -> Self {
        let local_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("engram")
            .join("images");
        Self {
            local_dir,
            s3_bucket: None,
            s3_endpoint: None,
            public_domain: None,
        }
    }
}

/// Uploaded image information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedImage {
    /// Storage key/path
    pub key: String,
    /// Full URL to access the image
    pub url: String,
    /// Original filename if available
    pub filename: Option<String>,
    /// Content type (MIME type)
    pub content_type: String,
    /// Size in bytes
    pub size: usize,
    /// Content hash (SHA256)
    pub hash: String,
}

/// Image reference stored in memory metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRef {
    /// Storage URL (local:// or r2:// or https://)
    pub url: String,
    /// Optional caption
    pub caption: Option<String>,
    /// Image index within the memory
    pub index: i32,
    /// Content type
    pub content_type: String,
    /// Size in bytes
    pub size: usize,
}

/// Result of image migration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub memories_scanned: i64,
    pub memories_with_images: i64,
    pub images_migrated: i64,
    pub images_failed: i64,
    pub errors: Vec<String>,
    pub dry_run: bool,
}

/// Report returned by `sync_to_cloud`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MediaSyncReport {
    /// Number of media assets examined
    pub assets_examined: i64,
    /// Number of assets already in the cloud (skipped)
    pub assets_already_synced: i64,
    /// Number of assets successfully uploaded
    pub assets_uploaded: i64,
    /// Number of upload failures
    pub assets_failed: i64,
    /// Errors encountered during sync
    pub errors: Vec<String>,
    /// Whether this was a dry run (no actual uploads)
    pub dry_run: bool,
}

/// Compute SHA256 hash of data
fn compute_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

/// Get file extension from content type
fn extension_from_content_type(content_type: &str) -> &str {
    match content_type {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => "bin",
    }
}

/// Detect content type from file extension
fn content_type_from_extension(ext: &str) -> &str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        _ => "application/octet-stream",
    }
}

/// Parse a data URI into bytes and content type
pub fn parse_data_uri(data_uri: &str) -> Result<(Vec<u8>, String)> {
    // Match data URI format: data:mime/type;base64,DATA
    if !data_uri.starts_with("data:") {
        return Err(EngramError::InvalidInput("Not a data URI".to_string()));
    }

    let rest = &data_uri[5..];
    let (content_type, data) = if let Some(semicolon_pos) = rest.find(';') {
        let ct = &rest[..semicolon_pos];
        let after_semicolon = &rest[semicolon_pos + 1..];

        if let Some(stripped) = after_semicolon.strip_prefix("base64,") {
            (ct.to_string(), stripped)
        } else {
            return Err(EngramError::InvalidInput(
                "Invalid data URI encoding".to_string(),
            ));
        }
    } else {
        return Err(EngramError::InvalidInput(
            "Invalid data URI format".to_string(),
        ));
    };

    let bytes = BASE64
        .decode(data)
        .map_err(|e| EngramError::InvalidInput(format!("Failed to decode base64: {}", e)))?;

    Ok((bytes, content_type))
}

/// Local file-based image storage (used when S3 is not configured)
pub struct LocalImageStorage {
    base_dir: PathBuf,
}

impl LocalImageStorage {
    pub fn new(base_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&base_dir)
            .map_err(|e| EngramError::Storage(format!("Failed to create image dir: {}", e)))?;
        Ok(Self { base_dir })
    }

    /// Generate storage key for an image
    fn generate_key(
        &self,
        memory_id: i64,
        image_index: i32,
        hash: &str,
        extension: &str,
    ) -> String {
        let timestamp = Utc::now().timestamp();
        let short_hash = &hash[..8.min(hash.len())];
        format!(
            "images/{}/{}_{}_{}.{}",
            memory_id, timestamp, image_index, short_hash, extension
        )
    }

    /// Upload image from bytes
    pub fn upload_image(
        &self,
        image_data: &[u8],
        content_type: &str,
        memory_id: i64,
        image_index: i32,
    ) -> Result<UploadedImage> {
        let hash = compute_hash(image_data);
        let extension = extension_from_content_type(content_type);
        let key = self.generate_key(memory_id, image_index, &hash, extension);

        // Create directory structure
        let full_path = self.base_dir.join(&key);
        if let Some(parent) = full_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| EngramError::Storage(format!("Failed to create dir: {}", e)))?;
        }

        // Write file
        std::fs::write(&full_path, image_data)
            .map_err(|e| EngramError::Storage(format!("Failed to write image: {}", e)))?;

        let url = format!("local://{}", key);

        Ok(UploadedImage {
            key,
            url,
            filename: None,
            content_type: content_type.to_string(),
            size: image_data.len(),
            hash,
        })
    }

    /// Upload image from file path
    pub fn upload_from_file(
        &self,
        file_path: &str,
        memory_id: i64,
        image_index: i32,
    ) -> Result<UploadedImage> {
        let path = std::path::Path::new(file_path);

        // Read file
        let image_data = std::fs::read(path)
            .map_err(|e| EngramError::Storage(format!("Failed to read file: {}", e)))?;

        // Detect content type from extension
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("bin");
        let content_type = content_type_from_extension(extension);

        let mut result = self.upload_image(&image_data, content_type, memory_id, image_index)?;
        result.filename = path.file_name().and_then(|n| n.to_str()).map(String::from);

        Ok(result)
    }

    /// Get full path for a key
    pub fn get_path(&self, key: &str) -> PathBuf {
        self.base_dir.join(key)
    }

    /// Delete an image
    pub fn delete_image(&self, key: &str) -> Result<bool> {
        let path = self.get_path(key);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| EngramError::Storage(format!("Failed to delete image: {}", e)))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Delete all images for a memory
    pub fn delete_memory_images(&self, memory_id: i64) -> Result<i64> {
        let dir = self.base_dir.join("images").join(memory_id.to_string());
        if !dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(&dir)
            .map_err(|e| EngramError::Storage(format!("Failed to read dir: {}", e)))?
        {
            let entry =
                entry.map_err(|e| EngramError::Storage(format!("Failed to read entry: {}", e)))?;
            if entry.path().is_file() {
                std::fs::remove_file(entry.path())
                    .map_err(|e| EngramError::Storage(format!("Failed to delete file: {}", e)))?;
                count += 1;
            }
        }

        // Remove empty directory
        let _ = std::fs::remove_dir(&dir);

        Ok(count)
    }
}

/// Upload an image to storage and link it to a memory
pub fn upload_image(
    conn: &Connection,
    storage: &LocalImageStorage,
    memory_id: i64,
    file_path: &str,
    image_index: i32,
    caption: Option<&str>,
) -> Result<ImageRef> {
    use crate::storage::queries::get_memory;

    // Verify memory exists
    let memory = get_memory(conn, memory_id)?;

    // Upload the image
    let uploaded = storage.upload_from_file(file_path, memory_id, image_index)?;

    // Create image reference
    let image_ref = ImageRef {
        url: uploaded.url.clone(),
        caption: caption.map(String::from),
        index: image_index,
        content_type: uploaded.content_type,
        size: uploaded.size,
    };

    // Update memory metadata with image reference
    let mut metadata = memory.metadata.clone();
    let images: Vec<ImageRef> = metadata
        .get("images")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let mut images: Vec<ImageRef> = images
        .into_iter()
        .filter(|i| i.index != image_index)
        .collect();
    images.push(image_ref.clone());
    images.sort_by_key(|i| i.index);

    metadata.insert("images".to_string(), serde_json::to_value(&images)?);
    let metadata_json = serde_json::to_string(&metadata)?;

    conn.execute(
        "UPDATE memories SET metadata = ?, updated_at = ? WHERE id = ?",
        params![metadata_json, Utc::now().to_rfc3339(), memory_id],
    )?;

    Ok(image_ref)
}

/// Migrate base64-encoded images to storage
pub fn migrate_images(
    conn: &Connection,
    storage: &LocalImageStorage,
    dry_run: bool,
) -> Result<MigrationResult> {
    use crate::storage::queries::get_memory;

    let mut result = MigrationResult {
        memories_scanned: 0,
        memories_with_images: 0,
        images_migrated: 0,
        images_failed: 0,
        errors: Vec::new(),
        dry_run,
    };

    // Find all memories
    let mut stmt = conn.prepare("SELECT id, metadata FROM memories WHERE valid_to IS NULL")?;

    let memory_ids: Vec<i64> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    for memory_id in memory_ids {
        result.memories_scanned += 1;

        let memory = match get_memory(conn, memory_id) {
            Ok(m) => m,
            Err(e) => {
                result
                    .errors
                    .push(format!("Failed to get memory {}: {}", memory_id, e));
                continue;
            }
        };

        // Check for images in metadata
        let images: Vec<serde_json::Value> = memory
            .metadata
            .get("images")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Also check content for inline data URIs
        let content_has_data_uri = memory.content.contains("data:image/");

        if images.is_empty() && !content_has_data_uri {
            continue;
        }

        result.memories_with_images += 1;

        // Process images in metadata
        let mut new_images: Vec<ImageRef> = Vec::new();
        let mut image_index = 0;

        for img in images {
            let url = img.get("url").and_then(|v| v.as_str()).unwrap_or("");

            // Skip if already migrated (not a data URI)
            if !url.starts_with("data:") {
                if let Ok(existing) = serde_json::from_value::<ImageRef>(img.clone()) {
                    new_images.push(existing);
                }
                continue;
            }

            // Parse and upload data URI
            match parse_data_uri(url) {
                Ok((data, content_type)) => {
                    if dry_run {
                        result.images_migrated += 1;
                        // In dry run, keep existing
                        if let Ok(existing) = serde_json::from_value::<ImageRef>(img.clone()) {
                            new_images.push(existing);
                        }
                    } else {
                        match storage.upload_image(&data, &content_type, memory_id, image_index) {
                            Ok(uploaded) => {
                                let caption = img
                                    .get("caption")
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                                new_images.push(ImageRef {
                                    url: uploaded.url,
                                    caption,
                                    index: image_index,
                                    content_type: uploaded.content_type,
                                    size: uploaded.size,
                                });
                                result.images_migrated += 1;
                            }
                            Err(e) => {
                                result.images_failed += 1;
                                result.errors.push(format!(
                                    "Failed to upload image {} for memory {}: {}",
                                    image_index, memory_id, e
                                ));
                                // Keep original on failure
                                if let Ok(existing) =
                                    serde_json::from_value::<ImageRef>(img.clone())
                                {
                                    new_images.push(existing);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    result.images_failed += 1;
                    result.errors.push(format!(
                        "Failed to parse data URI for memory {}: {}",
                        memory_id, e
                    ));
                    // Keep original on failure
                    if let Ok(existing) = serde_json::from_value::<ImageRef>(img.clone()) {
                        new_images.push(existing);
                    }
                }
            }
            image_index += 1;
        }

        // Update metadata with migrated images (unless dry run)
        if !dry_run && !new_images.is_empty() {
            let mut metadata = memory.metadata.clone();
            metadata.insert("images".to_string(), serde_json::to_value(&new_images)?);
            let metadata_json = serde_json::to_string(&metadata)?;

            if let Err(e) = conn.execute(
                "UPDATE memories SET metadata = ?, updated_at = ? WHERE id = ?",
                params![metadata_json, Utc::now().to_rfc3339(), memory_id],
            ) {
                result
                    .errors
                    .push(format!("Failed to update memory {}: {}", memory_id, e));
            }
        }
    }

    Ok(result)
}

// ── Cloud sync helpers ────────────────────────────────────────────────────────

/// Build the S3 key for a media asset.
///
/// Format: `media/{memory_id}/{file_hash}.{ext}`
pub fn build_cloud_key(memory_id: i64, file_hash: &str, mime_type: &str) -> String {
    let ext = extension_from_content_type(mime_type);
    // Use first 16 chars of the hash to keep keys short but still unique
    let short_hash = &file_hash[..file_hash.len().min(16)];
    format!("media/{}/{}.{}", memory_id, short_hash, ext)
}

/// Build the public cloud URL from a bucket, optional public domain, and key.
pub fn build_cloud_url(
    s3_bucket: &str,
    s3_endpoint: Option<&str>,
    public_domain: Option<&str>,
    key: &str,
) -> String {
    if let Some(domain) = public_domain {
        format!("https://{}/{}", domain.trim_end_matches('/'), key)
    } else if let Some(endpoint) = s3_endpoint {
        format!(
            "{}/{}/{}",
            endpoint.trim_end_matches('/'),
            s3_bucket,
            key
        )
    } else {
        format!("https://{}.s3.amazonaws.com/{}", s3_bucket, key)
    }
}

/// Returns true if `file_path` already points to a cloud URL (not a local path).
pub fn is_cloud_url(file_path: &str) -> bool {
    file_path.starts_with("https://")
        || file_path.starts_with("http://")
        || file_path.starts_with("s3://")
        || file_path.starts_with("r2://")
}

/// Synchronise local media assets to S3/R2 cloud storage.
///
/// Queries the `media_assets` table for rows whose `file_path` is a local file
/// (i.e. not yet uploaded), uploads each to cloud storage at
/// `media/{memory_id}/{file_hash}.{ext}`, and updates `file_path` in the row
/// with the resulting cloud URL.
///
/// This function is feature-gated by both `multimodal` and `cloud`.
/// It must be triggered explicitly — it does NOT run automatically.
///
/// # Arguments
/// * `conn`    — SQLite connection
/// * `config`  — Image storage configuration (S3 bucket, endpoint, public domain)
/// * `dry_run` — If `true`, no uploads or updates are performed
#[cfg(feature = "cloud")]
pub fn sync_to_cloud(
    conn: &Connection,
    config: &ImageStorageConfig,
    dry_run: bool,
) -> crate::error::Result<MediaSyncReport> {
    let bucket = match &config.s3_bucket {
        Some(b) => b.clone(),
        None => {
            return Err(crate::error::EngramError::Config(
                "s3_bucket must be configured for cloud media sync".to_string(),
            ));
        }
    };

    let mut report = MediaSyncReport {
        dry_run,
        ..Default::default()
    };

    // Query all media assets
    let mut stmt = conn.prepare(
        "SELECT id, memory_id, file_hash, file_path, mime_type FROM media_assets",
    )?;

    struct AssetRow {
        id: i64,
        memory_id: i64,
        file_hash: String,
        file_path: Option<String>,
        mime_type: Option<String>,
    }

    let assets: Vec<AssetRow> = stmt
        .query_map([], |row| {
            Ok(AssetRow {
                id: row.get(0)?,
                memory_id: row.get(1)?,
                file_hash: row.get(2)?,
                file_path: row.get(3)?,
                mime_type: row.get(4)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    report.assets_examined = assets.len() as i64;

    for asset in assets {
        let file_path = match &asset.file_path {
            Some(p) => p.clone(),
            None => {
                report.assets_failed += 1;
                report
                    .errors
                    .push(format!("Asset id={} has no file_path", asset.id));
                continue;
            }
        };

        // Skip already-synced assets
        if is_cloud_url(&file_path) {
            report.assets_already_synced += 1;
            continue;
        }

        let mime_type = asset.mime_type.as_deref().unwrap_or("application/octet-stream");
        let cloud_key = build_cloud_key(asset.memory_id, &asset.file_hash, mime_type);
        let cloud_url = build_cloud_url(
            &bucket,
            config.s3_endpoint.as_deref(),
            config.public_domain.as_deref(),
            &cloud_key,
        );

        if dry_run {
            // In dry-run mode, just count what would be uploaded
            report.assets_uploaded += 1;
            continue;
        }

        // Read the local file
        // Strip "local://" prefix if present
        let local_path = file_path
            .strip_prefix("local://")
            .unwrap_or(&file_path);

        let file_data = match std::fs::read(local_path) {
            Ok(d) => d,
            Err(e) => {
                report.assets_failed += 1;
                report.errors.push(format!(
                    "Failed to read '{}' for asset id={}: {}",
                    local_path, asset.id, e
                ));
                continue;
            }
        };

        // Upload to S3/R2
        match upload_bytes_to_s3_blocking(&file_data, &bucket, &cloud_key, mime_type, config) {
            Ok(()) => {
                // Update file_path in media_assets
                conn.execute(
                    "UPDATE media_assets SET file_path = ? WHERE id = ?",
                    rusqlite::params![cloud_url, asset.id],
                )?;
                report.assets_uploaded += 1;
            }
            Err(e) => {
                report.assets_failed += 1;
                report.errors.push(format!(
                    "Failed to upload asset id={}: {}",
                    asset.id, e
                ));
            }
        }
    }

    Ok(report)
}

/// Download media bytes from a cloud URL.
///
/// Supports `https://` and `s3://` URLs. Returns the raw bytes of the file.
pub fn download_from_cloud(file_url: &str) -> crate::error::Result<Vec<u8>> {
    if file_url.starts_with("local://") {
        let path = file_url.strip_prefix("local://").unwrap_or(file_url);
        return std::fs::read(path).map_err(|e| {
            crate::error::EngramError::Storage(format!(
                "Failed to read local media file '{}': {}",
                path, e
            ))
        });
    }

    if file_url.starts_with("https://") || file_url.starts_with("http://") {
        // Use a tokio runtime + reqwest async client for HTTP download
        #[cfg(any(feature = "cloud", feature = "multimodal"))]
        {
            let url = file_url.to_string();
            let rt = tokio::runtime::Runtime::new().map_err(|e| {
                crate::error::EngramError::Storage(format!(
                    "Failed to create async runtime for download: {}",
                    e
                ))
            })?;
            return rt.block_on(async {
                let client = reqwest::Client::new();
                let response = client.get(&url).send().await.map_err(|e| {
                    crate::error::EngramError::Storage(format!(
                        "Failed to download '{}': {}",
                        url, e
                    ))
                })?;
                if !response.status().is_success() {
                    return Err(crate::error::EngramError::Storage(format!(
                        "HTTP {} downloading '{}'",
                        response.status(),
                        url
                    )));
                }
                response.bytes().await.map(|b| b.to_vec()).map_err(|e| {
                    crate::error::EngramError::Storage(format!(
                        "Failed to read response body from '{}': {}",
                        url, e
                    ))
                })
            });
        }
        #[cfg(not(any(feature = "cloud", feature = "multimodal")))]
        {
            return Err(crate::error::EngramError::Config(
                "Downloading from cloud URLs requires the 'cloud' or 'multimodal' feature".to_string(),
            ));
        }
    }

    Err(crate::error::EngramError::InvalidInput(format!(
        "Unsupported media URL scheme: '{}'",
        file_url
    )))
}

/// Blocking S3/R2 upload via tokio runtime + aws-sdk-s3.
#[cfg(feature = "cloud")]
fn upload_bytes_to_s3_blocking(
    data: &[u8],
    bucket: &str,
    key: &str,
    content_type: &str,
    _config: &ImageStorageConfig,
) -> crate::error::Result<()> {
    // Use a short-lived Tokio runtime for the async SDK call
    let rt = tokio::runtime::Runtime::new().map_err(|e| {
        crate::error::EngramError::Storage(format!(
            "Failed to create async runtime for S3 upload: {}",
            e
        ))
    })?;

    rt.block_on(async {
        let sdk_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;
        let client = aws_sdk_s3::Client::new(&sdk_config);

        let body = aws_sdk_s3::primitives::ByteStream::from(data.to_vec());
        client
            .put_object()
            .bucket(bucket)
            .key(key)
            .content_type(content_type)
            .body(body)
            .send()
            .await
            .map_err(|e| {
                crate::error::EngramError::Storage(format!(
                    "S3 PutObject failed for key '{}': {}",
                    key, e
                ))
            })?;

        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_data_uri() {
        let data_uri = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";
        let (bytes, content_type) = parse_data_uri(data_uri).unwrap();
        assert_eq!(content_type, "image/png");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn test_local_storage() {
        let dir = tempdir().unwrap();
        let storage = LocalImageStorage::new(dir.path().to_path_buf()).unwrap();

        // Create a simple 1x1 PNG
        let png_data = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

        let result = storage.upload_image(&png_data, "image/png", 1, 0).unwrap();
        assert!(result.url.starts_with("local://"));
        assert_eq!(result.content_type, "image/png");
        assert_eq!(result.size, png_data.len());

        // Verify file exists
        let path = storage.get_path(&result.key);
        assert!(path.exists());

        // Delete
        assert!(storage.delete_image(&result.key).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn test_content_type_detection() {
        assert_eq!(content_type_from_extension("jpg"), "image/jpeg");
        assert_eq!(content_type_from_extension("PNG"), "image/png");
        assert_eq!(content_type_from_extension("webp"), "image/webp");
    }

    // ── T3: Cloud media sync tests ────────────────────────────────────────────

    #[test]
    fn test_build_cloud_key_image() {
        let key = build_cloud_key(42, "abcdef1234567890", "image/png");
        assert_eq!(key, "media/42/abcdef1234567890.png");
    }

    #[test]
    fn test_build_cloud_key_audio() {
        let key = build_cloud_key(7, "feedbeef12345678", "audio/mpeg");
        assert_eq!(key, "media/7/feedbeef12345678.bin");
    }

    #[test]
    fn test_build_cloud_url_with_public_domain() {
        let url = build_cloud_url(
            "my-bucket",
            None,
            Some("media.example.com"),
            "media/42/abc.png",
        );
        assert_eq!(url, "https://media.example.com/media/42/abc.png");
    }

    #[test]
    fn test_build_cloud_url_with_s3_endpoint() {
        let url = build_cloud_url(
            "my-bucket",
            Some("https://r2.example.com"),
            None,
            "media/42/abc.png",
        );
        assert_eq!(url, "https://r2.example.com/my-bucket/media/42/abc.png");
    }

    #[test]
    fn test_build_cloud_url_default_s3() {
        let url = build_cloud_url("my-bucket", None, None, "media/42/abc.png");
        assert_eq!(
            url,
            "https://my-bucket.s3.amazonaws.com/media/42/abc.png"
        );
    }

    #[test]
    fn test_is_cloud_url() {
        assert!(is_cloud_url("https://cdn.example.com/file.png"));
        assert!(is_cloud_url("http://cdn.example.com/file.png"));
        assert!(is_cloud_url("s3://my-bucket/file.png"));
        assert!(is_cloud_url("r2://my-bucket/file.png"));
        assert!(!is_cloud_url("local:///tmp/file.png"));
        assert!(!is_cloud_url("/tmp/file.png"));
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn test_sync_to_cloud_no_bucket_returns_error() {
        use crate::storage::migrations::run_migrations;

        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        run_migrations(&conn).expect("migrations");

        let config = ImageStorageConfig {
            local_dir: std::path::PathBuf::from("/tmp"),
            s3_bucket: None, // no bucket configured
            s3_endpoint: None,
            public_domain: None,
        };

        let result = sync_to_cloud(&conn, &config, true);
        assert!(result.is_err(), "should fail without bucket configured");
    }

    #[cfg(feature = "cloud")]
    #[test]
    fn test_sync_to_cloud_empty_table_dry_run() {
        use crate::storage::migrations::run_migrations;

        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        run_migrations(&conn).expect("migrations");

        let config = ImageStorageConfig {
            local_dir: std::path::PathBuf::from("/tmp"),
            s3_bucket: Some("test-bucket".to_string()),
            s3_endpoint: None,
            public_domain: None,
        };

        let report = sync_to_cloud(&conn, &config, true).expect("sync report");
        assert_eq!(report.assets_examined, 0);
        assert_eq!(report.assets_uploaded, 0);
        assert_eq!(report.assets_failed, 0);
        assert!(report.dry_run);
    }
}
