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
}
