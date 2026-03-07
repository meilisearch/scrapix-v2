//! # S3-Compatible Object Storage
//!
//! Object storage backend for raw HTML archives and large binary assets.
//!
//! Supports S3-compatible storage services:
//! - AWS S3
//! - MinIO
//! - RustFS
//! - Cloudflare R2
//! - DigitalOcean Spaces
//!
//! ## Example
//!
//! ```rust,ignore
//! use scrapix_storage::object_storage::{S3Storage, S3Config};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let storage = S3Storage::builder()
//!         .endpoint("http://localhost:9000")
//!         .bucket("scrapix-raw")
//!         .access_key("minioadmin")
//!         .secret_key("minioadmin")
//!         .region("us-east-1")
//!         .build()
//!         .await?;
//!
//!     // Store raw HTML
//!     let key = "pages/example.com/index.html";
//!     storage.put_object(key, b"<html>...</html>", Some("text/html")).await?;
//!
//!     // Retrieve it later
//!     let data = storage.get_object(key).await?;
//!
//!     // Check existence
//!     if storage.exists(key).await? {
//!         storage.delete_object(key).await?;
//!     }
//!
//!     Ok(())
//! }
//! ```

use bytes::Bytes;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::error::S3Error;
use s3::region::Region;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, instrument, warn};

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur during S3 operations.
#[derive(Error, Debug)]
pub enum ObjectStorageError {
    /// Failed to create credentials.
    #[error("Failed to create credentials: {0}")]
    CredentialsError(String),

    /// Failed to create bucket connection.
    #[error("Failed to create bucket: {0}")]
    BucketError(String),

    /// S3 operation failed.
    #[error("S3 operation failed: {0}")]
    S3Error(#[from] S3Error),

    /// Object not found.
    #[error("Object not found: {0}")]
    NotFound(String),

    /// Invalid configuration.
    #[error("Invalid configuration: {0}")]
    ConfigError(String),

    /// Serialization error.
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for S3-compatible object storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3Config {
    /// S3 endpoint URL (e.g., "http://localhost:9000" for MinIO).
    pub endpoint: String,

    /// Bucket name.
    pub bucket: String,

    /// AWS region or custom region name.
    #[serde(default = "default_region")]
    pub region: String,

    /// Access key ID.
    #[serde(default)]
    pub access_key: Option<String>,

    /// Secret access key.
    #[serde(default)]
    pub secret_key: Option<String>,

    /// Security token for temporary credentials (optional).
    #[serde(default)]
    pub security_token: Option<String>,

    /// Session token for temporary credentials (optional).
    #[serde(default)]
    pub session_token: Option<String>,

    /// Whether to use path-style URLs (required for MinIO, RustFS).
    #[serde(default = "default_true")]
    pub path_style: bool,

    /// Request timeout in seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Maximum number of retries for failed operations.
    #[serde(default = "default_retries")]
    pub max_retries: u32,

    /// Prefix for all object keys (optional).
    #[serde(default)]
    pub key_prefix: Option<String>,
}

fn default_region() -> String {
    "us-east-1".to_string()
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    30
}

fn default_retries() -> u32 {
    3
}

impl Default for S3Config {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:9000".to_string(),
            bucket: "scrapix".to_string(),
            region: default_region(),
            access_key: None,
            secret_key: None,
            security_token: None,
            session_token: None,
            path_style: true,
            timeout_secs: default_timeout(),
            max_retries: default_retries(),
            key_prefix: None,
        }
    }
}

impl S3Config {
    /// Create a new S3 configuration builder.
    pub fn builder() -> S3ConfigBuilder {
        S3ConfigBuilder::default()
    }

    /// Create configuration for MinIO with defaults.
    pub fn minio(endpoint: impl Into<String>, bucket: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            bucket: bucket.into(),
            path_style: true,
            ..Default::default()
        }
    }

    /// Create configuration for AWS S3.
    pub fn aws_s3(bucket: impl Into<String>, region: impl Into<String>) -> Self {
        Self {
            endpoint: String::new(), // Use default AWS endpoint
            bucket: bucket.into(),
            region: region.into(),
            path_style: false,
            ..Default::default()
        }
    }
}

/// Builder for S3Config.
#[derive(Debug, Default)]
pub struct S3ConfigBuilder {
    endpoint: Option<String>,
    bucket: Option<String>,
    region: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    security_token: Option<String>,
    session_token: Option<String>,
    path_style: Option<bool>,
    timeout_secs: Option<u64>,
    max_retries: Option<u32>,
    key_prefix: Option<String>,
}

impl S3ConfigBuilder {
    /// Set the S3 endpoint URL.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }

    /// Set the bucket name.
    pub fn bucket(mut self, bucket: impl Into<String>) -> Self {
        self.bucket = Some(bucket.into());
        self
    }

    /// Set the AWS region.
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Set the access key ID.
    pub fn access_key(mut self, key: impl Into<String>) -> Self {
        self.access_key = Some(key.into());
        self
    }

    /// Set the secret access key.
    pub fn secret_key(mut self, key: impl Into<String>) -> Self {
        self.secret_key = Some(key.into());
        self
    }

    /// Set the security token for temporary credentials.
    pub fn security_token(mut self, token: impl Into<String>) -> Self {
        self.security_token = Some(token.into());
        self
    }

    /// Set the session token for temporary credentials.
    pub fn session_token(mut self, token: impl Into<String>) -> Self {
        self.session_token = Some(token.into());
        self
    }

    /// Enable or disable path-style URLs.
    pub fn path_style(mut self, enabled: bool) -> Self {
        self.path_style = Some(enabled);
        self
    }

    /// Set the request timeout in seconds.
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Set the maximum number of retries.
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = Some(retries);
        self
    }

    /// Set a prefix for all object keys.
    pub fn key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.key_prefix = Some(prefix.into());
        self
    }

    /// Build the S3Config.
    pub fn build(self) -> Result<S3Config, ObjectStorageError> {
        let bucket = self
            .bucket
            .ok_or_else(|| ObjectStorageError::ConfigError("bucket is required".to_string()))?;

        Ok(S3Config {
            endpoint: self
                .endpoint
                .unwrap_or_else(|| "http://localhost:9000".to_string()),
            bucket,
            region: self.region.unwrap_or_else(default_region),
            access_key: self.access_key,
            secret_key: self.secret_key,
            security_token: self.security_token,
            session_token: self.session_token,
            path_style: self.path_style.unwrap_or(true),
            timeout_secs: self.timeout_secs.unwrap_or(default_timeout()),
            max_retries: self.max_retries.unwrap_or(default_retries()),
            key_prefix: self.key_prefix,
        })
    }
}

// ============================================================================
// S3 Storage
// ============================================================================

/// S3-compatible object storage client.
///
/// Provides methods for storing and retrieving objects from S3-compatible
/// storage services like AWS S3, MinIO, RustFS, and others.
#[derive(Clone)]
pub struct S3Storage {
    bucket: Arc<Bucket>,
    config: S3Config,
}

impl std::fmt::Debug for S3Storage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Storage")
            .field("bucket", &self.config.bucket)
            .field("endpoint", &self.config.endpoint)
            .field("region", &self.config.region)
            .finish()
    }
}

impl S3Storage {
    /// Create a new S3Storage builder.
    pub fn builder() -> S3StorageBuilder {
        S3StorageBuilder::default()
    }

    /// Create S3Storage from configuration.
    pub async fn from_config(config: S3Config) -> Result<Self, ObjectStorageError> {
        let credentials = Self::create_credentials(&config)?;
        let region = Self::create_region(&config);
        let bucket = Self::create_bucket(&config, region, credentials)?;

        Ok(Self {
            bucket: Arc::new(bucket),
            config,
        })
    }

    fn create_credentials(config: &S3Config) -> Result<Credentials, ObjectStorageError> {
        match (&config.access_key, &config.secret_key) {
            (Some(access), Some(secret)) => Credentials::new(
                Some(access),
                Some(secret),
                config.security_token.as_deref(),
                config.session_token.as_deref(),
                None,
            )
            .map_err(|e| ObjectStorageError::CredentialsError(e.to_string())),
            _ => {
                // Try to load from environment or IAM
                Credentials::default()
                    .map_err(|e| ObjectStorageError::CredentialsError(e.to_string()))
            }
        }
    }

    fn create_region(config: &S3Config) -> Region {
        if config.endpoint.is_empty() {
            // Use standard AWS region
            config.region.parse().unwrap_or(Region::UsEast1)
        } else {
            // Custom endpoint
            Region::Custom {
                region: config.region.clone(),
                endpoint: config.endpoint.clone(),
            }
        }
    }

    fn create_bucket(
        config: &S3Config,
        region: Region,
        credentials: Credentials,
    ) -> Result<Bucket, ObjectStorageError> {
        let bucket = Bucket::new(&config.bucket, region, credentials)
            .map_err(|e| ObjectStorageError::BucketError(e.to_string()))?;

        let bucket = if config.path_style {
            bucket.with_path_style()
        } else {
            bucket
        };

        Ok(bucket)
    }

    /// Get the full key with optional prefix.
    fn full_key(&self, key: &str) -> String {
        match &self.config.key_prefix {
            Some(prefix) => format!("{}/{}", prefix.trim_end_matches('/'), key),
            None => key.to_string(),
        }
    }

    /// Get the bucket name.
    pub fn bucket_name(&self) -> &str {
        &self.config.bucket
    }

    /// Get the endpoint URL.
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    /// Get the configuration.
    pub fn config(&self) -> &S3Config {
        &self.config
    }

    // ========================================================================
    // Core Operations
    // ========================================================================

    /// Store an object with optional content type.
    #[instrument(skip(self, data), fields(bucket = %self.config.bucket, key = %key, size = data.len()))]
    pub async fn put_object(
        &self,
        key: &str,
        data: &[u8],
        content_type: Option<&str>,
    ) -> Result<(), ObjectStorageError> {
        let full_key = self.full_key(key);
        let ct = content_type.unwrap_or("application/octet-stream");

        debug!(key = %full_key, content_type = %ct, "Storing object");

        self.bucket
            .put_object_with_content_type(&full_key, data, ct)
            .await?;

        Ok(())
    }

    /// Store an object from bytes with optional content type.
    pub async fn put_bytes(
        &self,
        key: &str,
        data: Bytes,
        content_type: Option<&str>,
    ) -> Result<(), ObjectStorageError> {
        self.put_object(key, &data, content_type).await
    }

    /// Retrieve an object as bytes.
    #[instrument(skip(self), fields(bucket = %self.config.bucket, key = %key))]
    pub async fn get_object(&self, key: &str) -> Result<Bytes, ObjectStorageError> {
        let full_key = self.full_key(key);

        debug!(key = %full_key, "Retrieving object");

        let response = self.bucket.get_object(&full_key).await?;

        if response.status_code() == 404 {
            return Err(ObjectStorageError::NotFound(key.to_string()));
        }

        let data: Vec<u8> = response.into();
        Ok(Bytes::from(data))
    }

    /// Delete an object.
    #[instrument(skip(self), fields(bucket = %self.config.bucket, key = %key))]
    pub async fn delete_object(&self, key: &str) -> Result<(), ObjectStorageError> {
        let full_key = self.full_key(key);

        debug!(key = %full_key, "Deleting object");

        self.bucket.delete_object(&full_key).await?;

        Ok(())
    }

    /// Check if an object exists.
    #[instrument(skip(self), fields(bucket = %self.config.bucket, key = %key))]
    pub async fn exists(&self, key: &str) -> Result<bool, ObjectStorageError> {
        let full_key = self.full_key(key);

        match self.bucket.head_object(&full_key).await {
            Ok(_) => Ok(true),
            Err(S3Error::HttpFailWithBody(404, _)) => Ok(false),
            Err(e) => Err(ObjectStorageError::S3Error(e)),
        }
    }

    /// Get object metadata without downloading the content.
    #[instrument(skip(self), fields(bucket = %self.config.bucket, key = %key))]
    pub async fn head_object(&self, key: &str) -> Result<ObjectMetadata, ObjectStorageError> {
        let full_key = self.full_key(key);

        let (head, status) = self.bucket.head_object(&full_key).await?;

        if status == 404 {
            return Err(ObjectStorageError::NotFound(key.to_string()));
        }

        Ok(ObjectMetadata {
            key: key.to_string(),
            size: head.content_length.unwrap_or(0) as u64,
            content_type: head.content_type,
            last_modified: head.last_modified,
            etag: head.e_tag,
        })
    }

    /// List objects with a given prefix.
    #[instrument(skip(self), fields(bucket = %self.config.bucket, prefix = %prefix))]
    pub async fn list_objects(
        &self,
        prefix: &str,
        max_keys: Option<usize>,
    ) -> Result<Vec<ObjectInfo>, ObjectStorageError> {
        let full_prefix = self.full_key(prefix);

        debug!(prefix = %full_prefix, "Listing objects");

        let results = self.bucket.list(full_prefix, None).await?;

        let mut objects = Vec::new();
        let limit = max_keys.unwrap_or(usize::MAX);

        for result in results {
            for obj in result.contents {
                if objects.len() >= limit {
                    break;
                }

                // Remove the prefix from the key if present
                let key = if let Some(ref prefix) = self.config.key_prefix {
                    obj.key
                        .strip_prefix(&format!("{}/", prefix.trim_end_matches('/')))
                        .unwrap_or(&obj.key)
                        .to_string()
                } else {
                    obj.key
                };

                objects.push(ObjectInfo {
                    key,
                    size: obj.size,
                    last_modified: Some(obj.last_modified),
                    etag: obj.e_tag,
                });
            }
        }

        Ok(objects)
    }

    /// Delete multiple objects at once.
    #[instrument(skip(self, keys), fields(bucket = %self.config.bucket, count = keys.len()))]
    pub async fn delete_objects(&self, keys: &[&str]) -> Result<usize, ObjectStorageError> {
        if keys.is_empty() {
            return Ok(0);
        }

        let full_keys: Vec<String> = keys.iter().map(|k| self.full_key(k)).collect();

        debug!(count = full_keys.len(), "Deleting multiple objects");

        let mut deleted = 0;
        for key in &full_keys {
            match self.bucket.delete_object(key).await {
                Ok(_) => deleted += 1,
                Err(e) => {
                    warn!(key = %key, error = %e, "Failed to delete object");
                }
            }
        }

        Ok(deleted)
    }

    // ========================================================================
    // Convenience Methods
    // ========================================================================

    /// Store a JSON-serializable object.
    #[instrument(skip(self, value), fields(bucket = %self.config.bucket, key = %key))]
    pub async fn put_json<T: Serialize>(
        &self,
        key: &str,
        value: &T,
    ) -> Result<(), ObjectStorageError> {
        let data = serde_json::to_vec(value).map_err(|e| {
            ObjectStorageError::ConfigError(format!(
                "Failed to serialize JSON for key '{}': {}",
                key, e
            ))
        })?;
        self.put_object(key, &data, Some("application/json")).await
    }

    /// Retrieve and deserialize a JSON object.
    #[instrument(skip(self), fields(bucket = %self.config.bucket, key = %key))]
    pub async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> Result<T, ObjectStorageError> {
        let data = self.get_object(key).await?;
        let value = serde_json::from_slice(&data).map_err(|e| {
            ObjectStorageError::ConfigError(format!(
                "Failed to deserialize JSON for key '{}': {}",
                key, e
            ))
        })?;
        Ok(value)
    }

    /// Store raw HTML content.
    pub async fn put_html(&self, key: &str, html: &str) -> Result<(), ObjectStorageError> {
        self.put_object(key, html.as_bytes(), Some("text/html; charset=utf-8"))
            .await
    }

    /// Retrieve raw HTML content.
    pub async fn get_html(&self, key: &str) -> Result<String, ObjectStorageError> {
        let data = self.get_object(key).await?;
        String::from_utf8(data.to_vec())
            .map_err(|e| ObjectStorageError::ConfigError(format!("Invalid UTF-8: {}", e)))
    }

    /// Generate a key for storing a page by URL.
    ///
    /// Creates a path like: `pages/{domain}/{path_hash}.html`
    pub fn url_to_key(url: &str) -> String {
        use sha2::{Digest, Sha256};

        let parsed = url::Url::parse(url).ok();
        let domain = parsed
            .as_ref()
            .and_then(|u| u.host_str())
            .unwrap_or("unknown");

        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let hash = hasher.finalize();
        let hash_hex = hex::encode(&hash[..8]); // First 8 bytes = 16 hex chars

        format!("pages/{}/{}.html", domain, hash_hex)
    }

    /// Store a page by URL.
    pub async fn put_page(&self, url: &str, html: &str) -> Result<String, ObjectStorageError> {
        let key = Self::url_to_key(url);
        self.put_html(&key, html).await?;
        Ok(key)
    }

    /// Retrieve a page by URL.
    pub async fn get_page(&self, url: &str) -> Result<String, ObjectStorageError> {
        let key = Self::url_to_key(url);
        self.get_html(&key).await
    }

    /// Check if a page exists by URL.
    pub async fn page_exists(&self, url: &str) -> Result<bool, ObjectStorageError> {
        let key = Self::url_to_key(url);
        self.exists(&key).await
    }
}

// ============================================================================
// Builder
// ============================================================================

/// Builder for S3Storage.
#[derive(Debug, Default)]
pub struct S3StorageBuilder {
    config: S3ConfigBuilder,
}

impl S3StorageBuilder {
    /// Set the S3 endpoint URL.
    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.config = self.config.endpoint(endpoint);
        self
    }

    /// Set the bucket name.
    pub fn bucket(mut self, bucket: impl Into<String>) -> Self {
        self.config = self.config.bucket(bucket);
        self
    }

    /// Set the AWS region.
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.config = self.config.region(region);
        self
    }

    /// Set the access key ID.
    pub fn access_key(mut self, key: impl Into<String>) -> Self {
        self.config = self.config.access_key(key);
        self
    }

    /// Set the secret access key.
    pub fn secret_key(mut self, key: impl Into<String>) -> Self {
        self.config = self.config.secret_key(key);
        self
    }

    /// Enable or disable path-style URLs.
    pub fn path_style(mut self, enabled: bool) -> Self {
        self.config = self.config.path_style(enabled);
        self
    }

    /// Set a prefix for all object keys.
    pub fn key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.config = self.config.key_prefix(prefix);
        self
    }

    /// Build the S3Storage.
    pub async fn build(self) -> Result<S3Storage, ObjectStorageError> {
        let config = self.config.build()?;
        S3Storage::from_config(config).await
    }
}

// ============================================================================
// Metadata Types
// ============================================================================

/// Metadata for an object in S3.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadata {
    /// Object key.
    pub key: String,

    /// Object size in bytes.
    pub size: u64,

    /// Content type (MIME type).
    pub content_type: Option<String>,

    /// Last modification time.
    pub last_modified: Option<String>,

    /// ETag (entity tag) for the object.
    pub etag: Option<String>,
}

/// Information about an object from listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectInfo {
    /// Object key.
    pub key: String,

    /// Object size in bytes.
    pub size: u64,

    /// Last modification time.
    pub last_modified: Option<String>,

    /// ETag (entity tag).
    pub etag: Option<String>,
}

// ============================================================================
// Hex encoding helper
// ============================================================================

mod hex {
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            s.push(HEX_CHARS[(b >> 4) as usize] as char);
            s.push(HEX_CHARS[(b & 0xf) as usize] as char);
        }
        s
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = S3Config::default();
        assert_eq!(config.endpoint, "http://localhost:9000");
        assert_eq!(config.bucket, "scrapix");
        assert_eq!(config.region, "us-east-1");
        assert!(config.path_style);
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_retries, 3);
    }

    #[test]
    fn test_minio_config() {
        let config = S3Config::minio("http://minio:9000", "test-bucket");
        assert_eq!(config.endpoint, "http://minio:9000");
        assert_eq!(config.bucket, "test-bucket");
        assert!(config.path_style);
    }

    #[test]
    fn test_aws_s3_config() {
        let config = S3Config::aws_s3("my-bucket", "eu-west-1");
        assert!(config.endpoint.is_empty());
        assert_eq!(config.bucket, "my-bucket");
        assert_eq!(config.region, "eu-west-1");
        assert!(!config.path_style);
    }

    #[test]
    fn test_config_builder() {
        let config = S3Config::builder()
            .endpoint("http://localhost:9000")
            .bucket("test-bucket")
            .region("us-west-2")
            .access_key("access")
            .secret_key("secret")
            .path_style(true)
            .timeout_secs(60)
            .max_retries(5)
            .key_prefix("prefix")
            .build()
            .unwrap();

        assert_eq!(config.endpoint, "http://localhost:9000");
        assert_eq!(config.bucket, "test-bucket");
        assert_eq!(config.region, "us-west-2");
        assert_eq!(config.access_key, Some("access".to_string()));
        assert_eq!(config.secret_key, Some("secret".to_string()));
        assert!(config.path_style);
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.key_prefix, Some("prefix".to_string()));
    }

    #[test]
    fn test_config_builder_missing_bucket() {
        let result = S3Config::builder()
            .endpoint("http://localhost:9000")
            .build();

        assert!(result.is_err());
        assert!(matches!(result, Err(ObjectStorageError::ConfigError(_))));
    }

    #[test]
    fn test_url_to_key() {
        let key = S3Storage::url_to_key("https://example.com/page/test");
        assert!(key.starts_with("pages/example.com/"));
        assert!(key.ends_with(".html"));

        // Same URL should produce same key
        let key2 = S3Storage::url_to_key("https://example.com/page/test");
        assert_eq!(key, key2);

        // Different URL should produce different key
        let key3 = S3Storage::url_to_key("https://example.com/page/other");
        assert_ne!(key, key3);
    }

    #[test]
    fn test_config_serialization() {
        let config = S3Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: S3Config = serde_json::from_str(&json).unwrap();

        assert_eq!(config.endpoint, deserialized.endpoint);
        assert_eq!(config.bucket, deserialized.bucket);
        assert_eq!(config.region, deserialized.region);
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex::encode(&[0x00]), "00");
        assert_eq!(hex::encode(&[0xff]), "ff");
        assert_eq!(hex::encode(&[0xab, 0xcd, 0xef]), "abcdef");
        assert_eq!(hex::encode(&[0x12, 0x34, 0x56, 0x78]), "12345678");
    }

    #[test]
    fn test_object_metadata_serialization() {
        let meta = ObjectMetadata {
            key: "test/key.html".to_string(),
            size: 1234,
            content_type: Some("text/html".to_string()),
            last_modified: Some("2024-01-15T10:00:00Z".to_string()),
            etag: Some("\"abc123\"".to_string()),
        };

        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: ObjectMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(meta.key, deserialized.key);
        assert_eq!(meta.size, deserialized.size);
        assert_eq!(meta.content_type, deserialized.content_type);
    }
}
