//! # Scrapix Storage
//!
//! Storage backends for Scrapix.
//!
//! ## Backends
//!
//! - **Meilisearch**: Primary store for search, metadata, and vectors
//! - **RocksDB**: Local key-value storage (per-worker state)
//! - **Redis/DragonflyDB**: Caching and rate limiting
//! - **RustFS/S3**: Object storage for raw HTML archive
//!
//! ## Example
//!
//! ```rust,no_run
//! use scrapix_storage::{MeilisearchStorageBuilder, RedisStorage, RocksStorage};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Meilisearch for document indexing
//!     let meili = MeilisearchStorageBuilder::new(
//!         "http://localhost:7700",
//!         "documents"
//!     )
//!     .api_key("masterKey")
//!     .build()
//!     .await?;
//!
//!     // Redis for rate limiting
//!     let redis = RedisStorage::with_url("redis://localhost:6379").await?;
//!
//!     // RocksDB for local state
//!     let rocks = RocksStorage::at_path("./data/rocksdb")?;
//!
//!     Ok(())
//! }
//! ```

pub mod clickhouse;
pub mod meilisearch;
pub mod object_storage;
pub mod redis;
pub mod rocks;

// Re-exports
pub use clickhouse::{
    AiUsageBatcher, AiUsageEvent, AiUsageStats, ClickHouseConfig, ClickHouseError,
    ClickHouseStorage, DailyStats, DomainStats, HourlyStats, JobEvent, JobEventBatcher,
    JobEventSummaryRow, JobStats, RequestEvent, RequestEventBatcher,
};
pub use meilisearch::{MeilisearchConfig, MeilisearchStorage, MeilisearchStorageBuilder};
pub use object_storage::{
    ObjectInfo, ObjectMetadata, ObjectStorageError, S3Config, S3ConfigBuilder, S3Storage,
    S3StorageBuilder,
};
pub use redis::{
    AcquireResult, RateLimiterConfig, RedisConfig, RedisRateLimiter, RedisSeenCache, RedisStorage,
};
pub use rocks::{RocksConfig, RocksSeenTracker, RocksStorage, RocksStorageAdapter, WorkerState};
