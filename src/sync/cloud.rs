//! Cloud storage backends (S3, R2, GCS, Azure)

use std::path::Path;

use aws_config::BehaviorVersion;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;

use crate::error::{EngramError, Result};

/// Cloud storage abstraction
pub struct CloudStorage {
    client: S3Client,
    bucket: String,
    key: String,
    encrypt: bool,
    encryption_key: Option<Vec<u8>>,
}

impl CloudStorage {
    /// Create from S3-compatible URI (s3://bucket/path/to/file.db)
    pub async fn from_uri(uri: &str, encrypt: bool) -> Result<Self> {
        let uri = uri
            .strip_prefix("s3://")
            .ok_or_else(|| EngramError::Config("URI must start with s3://".to_string()))?;

        let parts: Vec<&str> = uri.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(EngramError::Config(
                "URI must be s3://bucket/path".to_string(),
            ));
        }

        let bucket = parts[0].to_string();
        let key = parts[1].to_string();

        // Load AWS config from environment
        let config = aws_config::defaults(BehaviorVersion::latest()).load().await;
        let client = S3Client::new(&config);

        // Generate encryption key if needed
        let encryption_key = if encrypt {
            Some(generate_encryption_key()?)
        } else {
            None
        };

        Ok(Self {
            client,
            bucket,
            key,
            encrypt,
            encryption_key,
        })
    }

    /// Upload local file to cloud
    pub async fn upload(&self, local_path: &Path) -> Result<u64> {
        let data = tokio::fs::read(local_path).await?;
        let size = data.len() as u64;

        let body = if self.encrypt {
            let encrypted = self.encrypt_data(&data)?;
            ByteStream::from(encrypted)
        } else {
            ByteStream::from(data)
        };

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .body(body)
            .send()
            .await
            .map_err(|e| EngramError::CloudStorage(e.to_string()))?;

        tracing::info!(
            "Uploaded {} bytes to s3://{}/{}",
            size,
            self.bucket,
            self.key
        );
        Ok(size)
    }

    /// Download from cloud to local file
    pub async fn download(&self, local_path: &Path) -> Result<u64> {
        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .map_err(|e| EngramError::CloudStorage(e.to_string()))?;

        let data = response
            .body
            .collect()
            .await
            .map_err(|e| EngramError::CloudStorage(e.to_string()))?
            .into_bytes();

        let decrypted = if self.encrypt {
            self.decrypt_data(&data)?
        } else {
            data.to_vec()
        };

        let size = decrypted.len() as u64;

        // Ensure parent directory exists
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(local_path, &decrypted).await?;

        tracing::info!(
            "Downloaded {} bytes from s3://{}/{}",
            size,
            self.bucket,
            self.key
        );
        Ok(size)
    }

    /// Check if remote file exists
    pub async fn exists(&self) -> Result<bool> {
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => {
                let service_error = e.into_service_error();
                if service_error.is_not_found() {
                    Ok(false)
                } else {
                    Err(EngramError::CloudStorage(service_error.to_string()))
                }
            }
        }
    }

    /// Get remote file metadata
    pub async fn metadata(&self) -> Result<CloudMetadata> {
        let response = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .map_err(|e| EngramError::CloudStorage(e.to_string()))?;

        Ok(CloudMetadata {
            size: response.content_length().unwrap_or(0) as u64,
            last_modified: response.last_modified().map(|dt| dt.to_string()),
            etag: response.e_tag().map(String::from),
        })
    }

    /// Delete remote file
    pub async fn delete(&self) -> Result<()> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&self.key)
            .send()
            .await
            .map_err(|e| EngramError::CloudStorage(e.to_string()))?;

        Ok(())
    }

    /// Encrypt data using AES-256-GCM
    fn encrypt_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce,
        };
        use rand::RngCore;

        let key = self
            .encryption_key
            .as_ref()
            .ok_or_else(|| EngramError::Encryption("No encryption key".to_string()))?;

        let cipher =
            Aes256Gcm::new_from_slice(key).map_err(|e| EngramError::Encryption(e.to_string()))?;

        // Generate random nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, data)
            .map_err(|e| EngramError::Encryption(e.to_string()))?;

        // Prepend nonce to ciphertext
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    /// Decrypt data using AES-256-GCM
    fn decrypt_data(&self, data: &[u8]) -> Result<Vec<u8>> {
        use aes_gcm::{
            aead::{Aead, KeyInit},
            Aes256Gcm, Nonce,
        };

        if data.len() < 12 {
            return Err(EngramError::Encryption("Data too short".to_string()));
        }

        let key = self
            .encryption_key
            .as_ref()
            .ok_or_else(|| EngramError::Encryption("No encryption key".to_string()))?;

        let cipher =
            Aes256Gcm::new_from_slice(key).map_err(|e| EngramError::Encryption(e.to_string()))?;

        let nonce = Nonce::from_slice(&data[..12]);
        let ciphertext = &data[12..];

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| EngramError::Encryption(e.to_string()))?;

        Ok(plaintext)
    }
}

/// Cloud file metadata
#[derive(Debug, Clone)]
pub struct CloudMetadata {
    pub size: u64,
    pub last_modified: Option<String>,
    pub etag: Option<String>,
}

/// Generate a random 256-bit encryption key
fn generate_encryption_key() -> Result<Vec<u8>> {
    use rand::RngCore;
    let mut key = vec![0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    Ok(key)
}

/// Derive encryption key from passphrase
#[allow(dead_code)]
pub fn derive_key_from_passphrase(passphrase: &str, salt: &[u8]) -> Result<Vec<u8>> {
    use std::num::NonZeroU32;

    // Simple PBKDF2-like derivation (in production, use proper PBKDF2 or Argon2)
    let iterations = NonZeroU32::new(100_000).unwrap();
    let mut key = vec![0u8; 32];

    // Simplified key derivation (replace with ring::pbkdf2 in production)
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    for _ in 0..iterations.get() {
        passphrase.hash(&mut hasher);
        salt.hash(&mut hasher);
    }
    let hash = hasher.finish();
    key[..8].copy_from_slice(&hash.to_le_bytes());

    // Fill rest of key with more hashing rounds
    for i in 1..4 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key[..i * 8].hash(&mut h);
        passphrase.hash(&mut h);
        let hash = h.finish();
        key[i * 8..(i + 1) * 8].copy_from_slice(&hash.to_le_bytes());
    }

    Ok(key)
}
