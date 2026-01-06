//! RocksDB storage backend for local per-worker state

use std::path::Path;
use std::sync::Arc;

use rocksdb::{ColumnFamilyDescriptor, IteratorMode, Options, DB};
use serde::{de::DeserializeOwned, Serialize};

use scrapix_core::{Result, ScrapixError};

/// RocksDB configuration
#[derive(Debug, Clone)]
pub struct RocksConfig {
    /// Database path
    pub path: String,
    /// Create if missing
    pub create_if_missing: bool,
    /// Column families to create
    pub column_families: Vec<String>,
    /// Write buffer size (bytes)
    pub write_buffer_size: usize,
    /// Max write buffer number
    pub max_write_buffer_number: i32,
    /// Target file size base (bytes)
    pub target_file_size_base: u64,
    /// Max background jobs
    pub max_background_jobs: i32,
    /// Enable compression
    pub enable_compression: bool,
}

impl Default for RocksConfig {
    fn default() -> Self {
        Self {
            path: "./data/rocksdb".to_string(),
            create_if_missing: true,
            column_families: vec![
                "default".to_string(),
                "seen_urls".to_string(),
                "state".to_string(),
            ],
            write_buffer_size: 64 * 1024 * 1024, // 64MB
            max_write_buffer_number: 3,
            target_file_size_base: 64 * 1024 * 1024, // 64MB
            max_background_jobs: 4,
            enable_compression: true,
        }
    }
}

/// RocksDB storage backend
pub struct RocksStorage {
    db: Arc<DB>,
    #[allow(dead_code)]
    config: RocksConfig,
}

impl RocksStorage {
    /// Create a new RocksDB storage
    pub fn new(config: RocksConfig) -> Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(config.create_if_missing);
        opts.create_missing_column_families(true);
        opts.set_write_buffer_size(config.write_buffer_size);
        opts.set_max_write_buffer_number(config.max_write_buffer_number);
        opts.set_target_file_size_base(config.target_file_size_base);
        opts.set_max_background_jobs(config.max_background_jobs);

        if config.enable_compression {
            opts.set_compression_type(rocksdb::DBCompressionType::Lz4);
        }

        // Open with column families
        let cf_descriptors: Vec<ColumnFamilyDescriptor> = config
            .column_families
            .iter()
            .map(|name| {
                let cf_opts = Options::default();
                ColumnFamilyDescriptor::new(name, cf_opts)
            })
            .collect();

        let db = if Path::new(&config.path).exists() {
            DB::open_cf_descriptors(&opts, &config.path, cf_descriptors)
        } else {
            // Create directory if needed
            std::fs::create_dir_all(&config.path)
                .map_err(|e| ScrapixError::Storage(format!("Failed to create directory: {}", e)))?;
            DB::open_cf_descriptors(&opts, &config.path, cf_descriptors)
        }
        .map_err(|e| ScrapixError::Storage(format!("Failed to open RocksDB: {}", e)))?;

        Ok(Self {
            db: Arc::new(db),
            config,
        })
    }

    /// Create storage at a specific path with defaults
    pub fn at_path(path: impl Into<String>) -> Result<Self> {
        Self::new(RocksConfig {
            path: path.into(),
            ..Default::default()
        })
    }

    /// Get a value by key from default column family
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.db
            .get(key)
            .map_err(|e| ScrapixError::Storage(format!("RocksDB GET failed: {}", e)))
    }

    /// Get a value from a specific column family
    pub fn get_cf(&self, cf_name: &str, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            ScrapixError::Storage(format!("Column family not found: {}", cf_name))
        })?;

        self.db
            .get_cf(&cf, key)
            .map_err(|e| ScrapixError::Storage(format!("RocksDB GET failed: {}", e)))
    }

    /// Set a value in default column family
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.db
            .put(key, value)
            .map_err(|e| ScrapixError::Storage(format!("RocksDB PUT failed: {}", e)))
    }

    /// Set a value in a specific column family
    pub fn put_cf(&self, cf_name: &str, key: &[u8], value: &[u8]) -> Result<()> {
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            ScrapixError::Storage(format!("Column family not found: {}", cf_name))
        })?;

        self.db
            .put_cf(&cf, key, value)
            .map_err(|e| ScrapixError::Storage(format!("RocksDB PUT failed: {}", e)))
    }

    /// Delete a key from default column family
    pub fn delete(&self, key: &[u8]) -> Result<()> {
        self.db
            .delete(key)
            .map_err(|e| ScrapixError::Storage(format!("RocksDB DELETE failed: {}", e)))
    }

    /// Delete a key from a specific column family
    pub fn delete_cf(&self, cf_name: &str, key: &[u8]) -> Result<()> {
        let cf = self.db.cf_handle(cf_name).ok_or_else(|| {
            ScrapixError::Storage(format!("Column family not found: {}", cf_name))
        })?;

        self.db
            .delete_cf(&cf, key)
            .map_err(|e| ScrapixError::Storage(format!("RocksDB DELETE failed: {}", e)))
    }

    /// Check if a key exists in default column family
    pub fn exists(&self, key: &[u8]) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    /// Check if a key exists in a specific column family
    pub fn exists_cf(&self, cf_name: &str, key: &[u8]) -> Result<bool> {
        Ok(self.get_cf(cf_name, key)?.is_some())
    }

    /// Get a JSON-serialized value
    pub fn get_json<T: DeserializeOwned>(&self, key: &[u8]) -> Result<Option<T>> {
        match self.get(key)? {
            Some(data) => {
                let value = serde_json::from_slice(&data)
                    .map_err(|e| ScrapixError::Storage(format!("Deserialization failed: {}", e)))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Set a JSON-serialized value
    pub fn put_json<T: Serialize>(&self, key: &[u8], value: &T) -> Result<()> {
        let data = serde_json::to_vec(value)
            .map_err(|e| ScrapixError::Storage(format!("Serialization failed: {}", e)))?;
        self.put(key, &data)
    }

    /// Iterate over all keys in default column family
    pub fn iter(&self) -> impl Iterator<Item = (Box<[u8]>, Box<[u8]>)> + '_ {
        self.db.iterator(IteratorMode::Start).filter_map(|r| r.ok())
    }

    /// Iterate over keys with a prefix
    pub fn prefix_iter(&self, prefix: &[u8]) -> impl Iterator<Item = (Box<[u8]>, Box<[u8]>)> + '_ {
        self.db.prefix_iterator(prefix).filter_map(|r| r.ok())
    }

    /// Count keys in default column family
    pub fn count(&self) -> usize {
        self.iter().count()
    }

    /// Count keys with a prefix
    pub fn count_prefix(&self, prefix: &[u8]) -> usize {
        self.prefix_iter(prefix).count()
    }

    /// Flush to disk
    pub fn flush(&self) -> Result<()> {
        self.db
            .flush()
            .map_err(|e| ScrapixError::Storage(format!("RocksDB FLUSH failed: {}", e)))
    }

    /// Get database statistics
    pub fn stats(&self) -> Option<String> {
        self.db.property_value("rocksdb.stats").ok().flatten()
    }

    /// Compact the database
    pub fn compact(&self) {
        self.db.compact_range::<&[u8], &[u8]>(None, None);
    }
}

/// URL seen tracker using RocksDB
pub struct RocksSeenTracker {
    storage: RocksStorage,
    cf_name: String,
}

impl RocksSeenTracker {
    /// Create a new seen tracker
    pub fn new(storage: RocksStorage, cf_name: impl Into<String>) -> Self {
        Self {
            storage,
            cf_name: cf_name.into(),
        }
    }

    /// Create with default column family
    pub fn with_defaults(storage: RocksStorage) -> Self {
        Self::new(storage, "seen_urls")
    }

    /// Mark a URL as seen
    pub fn mark_seen(&self, url: &str) -> Result<()> {
        let key = url_to_key(url);
        self.storage.put_cf(&self.cf_name, &key, &[1])
    }

    /// Check if a URL has been seen
    pub fn is_seen(&self, url: &str) -> Result<bool> {
        let key = url_to_key(url);
        self.storage.exists_cf(&self.cf_name, &key)
    }

    /// Mark multiple URLs as seen
    pub fn mark_seen_batch(&self, urls: &[String]) -> Result<()> {
        for url in urls {
            self.mark_seen(url)?;
        }
        Ok(())
    }

    /// Get count of seen URLs
    pub fn count(&self) -> Result<u64> {
        // This is expensive for large datasets
        // Consider maintaining a counter separately
        Ok(self.storage.count() as u64)
    }
}

/// Convert URL to a compact key
fn url_to_key(url: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    hasher.finalize().to_vec()
}

/// Worker state storage using RocksDB
pub struct WorkerState {
    storage: RocksStorage,
    cf_name: String,
}

impl WorkerState {
    /// Create a new worker state storage
    pub fn new(storage: RocksStorage, cf_name: impl Into<String>) -> Self {
        Self {
            storage,
            cf_name: cf_name.into(),
        }
    }

    /// Create with default column family
    pub fn with_defaults(storage: RocksStorage) -> Self {
        Self::new(storage, "state")
    }

    /// Get a state value
    pub fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        match self.storage.get_cf(&self.cf_name, key.as_bytes())? {
            Some(data) => {
                let value = serde_json::from_slice(&data)
                    .map_err(|e| ScrapixError::Storage(format!("Deserialization failed: {}", e)))?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    /// Set a state value
    pub fn set<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let data = serde_json::to_vec(value)
            .map_err(|e| ScrapixError::Storage(format!("Serialization failed: {}", e)))?;
        self.storage.put_cf(&self.cf_name, key.as_bytes(), &data)
    }

    /// Delete a state value
    pub fn delete(&self, key: &str) -> Result<()> {
        self.storage.delete_cf(&self.cf_name, key.as_bytes())
    }

    /// Check if a state key exists
    pub fn exists(&self, key: &str) -> Result<bool> {
        self.storage.exists_cf(&self.cf_name, key.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_storage() -> RocksStorage {
        let dir = TempDir::new().unwrap();
        RocksStorage::at_path(dir.path().to_str().unwrap()).unwrap()
    }

    #[test]
    fn test_basic_operations() {
        let storage = temp_storage();

        // Put and get
        storage.put(b"key1", b"value1").unwrap();
        assert_eq!(storage.get(b"key1").unwrap(), Some(b"value1".to_vec()));

        // Exists
        assert!(storage.exists(b"key1").unwrap());
        assert!(!storage.exists(b"key2").unwrap());

        // Delete
        storage.delete(b"key1").unwrap();
        assert!(!storage.exists(b"key1").unwrap());
    }

    #[test]
    fn test_json_operations() {
        let storage = temp_storage();

        #[derive(Debug, Serialize, serde::Deserialize, PartialEq)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        storage.put_json(b"json_key", &data).unwrap();
        let loaded: TestData = storage.get_json(b"json_key").unwrap().unwrap();
        assert_eq!(loaded, data);
    }

    #[test]
    fn test_seen_tracker() {
        let storage = temp_storage();
        let tracker = RocksSeenTracker::with_defaults(storage);

        assert!(!tracker.is_seen("https://example.com/page1").unwrap());

        tracker.mark_seen("https://example.com/page1").unwrap();
        assert!(tracker.is_seen("https://example.com/page1").unwrap());
        assert!(!tracker.is_seen("https://example.com/page2").unwrap());
    }

    #[test]
    fn test_url_to_key() {
        let key1 = url_to_key("https://example.com/page1");
        let key2 = url_to_key("https://example.com/page2");
        let key3 = url_to_key("https://example.com/page1");

        assert_ne!(key1, key2);
        assert_eq!(key1, key3);
        assert_eq!(key1.len(), 32); // SHA-256 produces 32 bytes
    }
}
