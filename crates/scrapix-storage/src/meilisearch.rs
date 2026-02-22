//! Meilisearch storage backend
//!
//! Primary storage for documents with full-text search, metadata, and vector capabilities.

use std::time::Duration;

use async_trait::async_trait;
use meilisearch_sdk::{
    client::{Client, SwapIndexes},
    indexes::Index,
    settings::{PaginationSetting, Settings},
    task_info::TaskInfo,
    tasks::TasksSearchQuery,
};
use tracing::{debug, info, instrument, warn};

use scrapix_core::{Document, Result, ScrapixError};

/// Meilisearch configuration
#[derive(Debug, Clone)]
pub struct MeilisearchConfig {
    /// Meilisearch URL
    pub url: String,
    /// API key
    pub api_key: Option<String>,
    /// Index UID
    pub index_uid: String,
    /// Primary key field
    pub primary_key: String,
    /// Searchable attributes
    pub searchable_attributes: Vec<String>,
    /// Filterable attributes
    pub filterable_attributes: Vec<String>,
    /// Sortable attributes
    pub sortable_attributes: Vec<String>,
    /// Displayed attributes (None = all)
    pub displayed_attributes: Option<Vec<String>>,
    /// Ranking rules
    pub ranking_rules: Option<Vec<String>>,
    /// Max total hits for pagination
    pub max_total_hits: usize,
    /// Batch size for document indexing
    pub batch_size: usize,
    /// Timeout for operations
    pub timeout: Duration,
}

impl Default for MeilisearchConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:7700".to_string(),
            api_key: None,
            index_uid: "documents".to_string(),
            primary_key: "uid".to_string(),
            searchable_attributes: vec![
                "title".to_string(),
                "content".to_string(),
                "markdown".to_string(),
                "h1".to_string(),
                "h2".to_string(),
                "h3".to_string(),
            ],
            filterable_attributes: vec![
                "domain".to_string(),
                "urls_tags".to_string(),
                "language".to_string(),
                "crawled_at".to_string(),
            ],
            sortable_attributes: vec!["crawled_at".to_string()],
            displayed_attributes: None,
            ranking_rules: None,
            max_total_hits: 10000,
            batch_size: 1000,
            timeout: Duration::from_secs(60),
        }
    }
}

/// Meilisearch storage backend
pub struct MeilisearchStorage {
    client: Client,
    config: MeilisearchConfig,
    index: Index,
    pending_docs: parking_lot::Mutex<Vec<Document>>,
}

impl MeilisearchStorage {
    /// Create a new Meilisearch storage client
    pub async fn new(config: MeilisearchConfig) -> Result<Self> {
        let client = Client::new(&config.url, config.api_key.as_deref()).map_err(|e| {
            ScrapixError::Storage(format!("Failed to create Meilisearch client: {}", e))
        })?;

        // Get or create index
        let index = client.index(&config.index_uid);

        let storage = Self {
            client,
            config,
            index,
            pending_docs: parking_lot::Mutex::new(Vec::new()),
        };

        // Initialize index settings
        storage.initialize_index().await?;

        Ok(storage)
    }

    /// Initialize index with configured settings
    #[instrument(skip(self))]
    async fn initialize_index(&self) -> Result<()> {
        // Create index if it doesn't exist
        let task = self
            .client
            .create_index(&self.config.index_uid, Some(&self.config.primary_key))
            .await
            .map_err(|e| ScrapixError::Storage(format!("Failed to create index: {}", e)))?;

        // Wait for index creation (might already exist, which is fine)
        let _ = self.wait_for_task(task).await;

        // Configure settings
        let mut settings = Settings::new();

        settings = settings
            .with_searchable_attributes(&self.config.searchable_attributes)
            .with_filterable_attributes(&self.config.filterable_attributes)
            .with_sortable_attributes(&self.config.sortable_attributes)
            .with_pagination(PaginationSetting {
                max_total_hits: self.config.max_total_hits,
            });

        if let Some(ref displayed) = self.config.displayed_attributes {
            settings = settings.with_displayed_attributes(displayed);
        }

        if let Some(ref ranking) = self.config.ranking_rules {
            settings = settings.with_ranking_rules(ranking);
        }

        let task =
            self.index.set_settings(&settings).await.map_err(|e| {
                ScrapixError::Storage(format!("Failed to set index settings: {}", e))
            })?;

        self.wait_for_task(task).await?;

        info!(
            index = %self.config.index_uid,
            "Meilisearch index initialized"
        );

        Ok(())
    }

    /// Add a single document to the index
    #[instrument(skip(self, doc), fields(url = %doc.url))]
    pub async fn add_document(&self, doc: Document) -> Result<()> {
        // Extract docs to flush in a separate scope
        let docs_to_flush = {
            let mut pending = self.pending_docs.lock();
            pending.push(doc);

            // Check if we need to flush
            if pending.len() >= self.config.batch_size {
                Some(std::mem::take(&mut *pending))
            } else {
                None
            }
        };

        // Flush outside the lock
        if let Some(docs) = docs_to_flush {
            self.index_documents(docs).await?;
        }

        Ok(())
    }

    /// Add multiple documents to the index
    pub async fn add_documents(&self, docs: Vec<Document>) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }

        // Extract docs to flush in a separate scope
        let docs_to_flush = {
            let mut pending = self.pending_docs.lock();
            pending.extend(docs);

            // Check if we need to flush
            if pending.len() >= self.config.batch_size {
                Some(std::mem::take(&mut *pending))
            } else {
                None
            }
        };

        // Flush outside the lock
        if let Some(docs) = docs_to_flush {
            self.index_documents(docs).await?;
        }

        Ok(())
    }

    /// Flush pending documents to the index
    /// Returns the number of documents flushed
    pub async fn flush(&self) -> Result<usize> {
        let docs = {
            let mut pending = self.pending_docs.lock();
            std::mem::take(&mut *pending)
        };

        let count = docs.len();
        if !docs.is_empty() {
            self.index_documents(docs).await?;
        }

        Ok(count)
    }

    /// Index documents (internal, fire-and-forget)
    ///
    /// Submits documents to Meilisearch and returns immediately without waiting
    /// for the indexing task to complete. Meilisearch processes tasks asynchronously.
    /// Use `wait_for_task()` if you need to confirm completion.
    async fn index_documents(&self, docs: Vec<Document>) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }

        let count = docs.len();
        debug!(count, "Submitting documents to Meilisearch");

        let task = self
            .index
            .add_documents(&docs, Some(&self.config.primary_key))
            .await
            .map_err(|e| ScrapixError::Storage(format!("Failed to add documents: {}", e)))?;

        info!(
            count,
            task_uid = task.task_uid,
            index = %self.config.index_uid,
            "Documents submitted to Meilisearch (fire-and-forget)"
        );

        Ok(())
    }

    /// Add a document directly to a specific index (bypasses batching)
    /// This is used when messages specify their own index_uid
    #[instrument(skip(self, doc), fields(url = %doc.url, index = %index_uid))]
    pub async fn add_document_to_index(&self, doc: Document, index_uid: &str) -> Result<()> {
        // Use the default index if the index_uid matches
        if index_uid == self.config.index_uid {
            return self.add_document(doc).await;
        }

        // Get or create the target index
        let index = self.client.index(index_uid);

        // Create the index if it doesn't exist (fire and forget, might already exist)
        let _ = self
            .client
            .create_index(index_uid, Some(&self.config.primary_key))
            .await;

        // Configure index settings (same as default index)
        let mut settings = Settings::new();
        settings = settings
            .with_searchable_attributes(&self.config.searchable_attributes)
            .with_filterable_attributes(&self.config.filterable_attributes)
            .with_sortable_attributes(&self.config.sortable_attributes)
            .with_pagination(PaginationSetting {
                max_total_hits: self.config.max_total_hits,
            });

        let _ = index.set_settings(&settings).await;

        // Index the document (fire-and-forget)
        let task = index
            .add_documents(&[doc], Some(&self.config.primary_key))
            .await
            .map_err(|e| {
                ScrapixError::Storage(format!("Failed to add document to {}: {}", index_uid, e))
            })?;

        debug!(
            task_uid = task.task_uid,
            index = %index_uid,
            "Document submitted to specific index (fire-and-forget)"
        );

        Ok(())
    }

    /// Add multiple documents directly to a specific index (bypasses batching)
    pub async fn add_documents_to_index(&self, docs: Vec<Document>, index_uid: &str) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }

        // Use the default index if the index_uid matches
        if index_uid == self.config.index_uid {
            return self.add_documents(docs).await;
        }

        let count = docs.len();

        // Get or create the target index
        let index = self.client.index(index_uid);

        // Create the index if it doesn't exist
        let _ = self
            .client
            .create_index(index_uid, Some(&self.config.primary_key))
            .await;

        // Index the documents (fire-and-forget)
        let task = index
            .add_documents(&docs, Some(&self.config.primary_key))
            .await
            .map_err(|e| {
                ScrapixError::Storage(format!("Failed to add documents to {}: {}", index_uid, e))
            })?;

        info!(
            count,
            task_uid = task.task_uid,
            index = %index_uid,
            "Documents submitted to specific index (fire-and-forget)"
        );

        Ok(())
    }

    /// Wait for a Meilisearch task to complete by polling.
    ///
    /// This is public so callers can optionally wait for task completion
    /// in cases where it matters (e.g., shutdown flushes, index initialization).
    /// For normal indexing operations, tasks are submitted fire-and-forget.
    pub async fn wait_for_task(&self, task_info: TaskInfo) -> Result<()> {
        loop {
            let task =
                self.client.get_task(&task_info).await.map_err(|e| {
                    ScrapixError::Storage(format!("Failed to get task status: {}", e))
                })?;

            match task {
                meilisearch_sdk::tasks::Task::Succeeded { .. } => {
                    return Ok(());
                }
                meilisearch_sdk::tasks::Task::Failed { content } => {
                    return Err(ScrapixError::Storage(format!(
                        "Task failed: {:?}",
                        content.error
                    )));
                }
                _ => {
                    // Still processing (Enqueued or Processing)
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }

    /// Get document count in the index
    pub async fn count(&self) -> Result<u64> {
        let stats = self
            .index
            .get_stats()
            .await
            .map_err(|e| ScrapixError::Storage(format!("Failed to get stats: {}", e)))?;

        Ok(stats.number_of_documents as u64)
    }

    /// Delete a document by UID
    pub async fn delete(&self, uid: &str) -> Result<()> {
        let task = self
            .index
            .delete_document(uid)
            .await
            .map_err(|e| ScrapixError::Storage(format!("Failed to delete document: {}", e)))?;

        self.wait_for_task(task).await
    }

    /// Delete all documents
    pub async fn delete_all(&self) -> Result<()> {
        let task =
            self.index.delete_all_documents().await.map_err(|e| {
                ScrapixError::Storage(format!("Failed to delete all documents: {}", e))
            })?;

        self.wait_for_task(task).await
    }

    /// Search for documents
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<Document>> {
        let results = self
            .index
            .search()
            .with_query(query)
            .with_limit(limit)
            .execute::<Document>()
            .await
            .map_err(|e| ScrapixError::Storage(format!("Search failed: {}", e)))?;

        Ok(results.hits.into_iter().map(|h| h.result).collect())
    }

    /// Get a document by UID
    pub async fn get(&self, uid: &str) -> Result<Option<Document>> {
        match self.index.get_document::<Document>(uid).await {
            Ok(doc) => Ok(Some(doc)),
            Err(meilisearch_sdk::errors::Error::Meilisearch(e))
                if e.error_code == meilisearch_sdk::errors::ErrorCode::DocumentNotFound =>
            {
                Ok(None)
            }
            Err(e) => Err(ScrapixError::Storage(format!(
                "Failed to get document: {}",
                e
            ))),
        }
    }

    /// Get index health status
    pub async fn health(&self) -> Result<bool> {
        match self.client.health().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Perform an atomic index swap between two indexes, then delete the old one.
    ///
    /// This is a static helper that creates a throwaway Meilisearch client,
    /// waits for all pending tasks on the temp index to settle, performs the
    /// atomic swap, and deletes the temp index (which now holds old data).
    pub async fn perform_swap(
        meilisearch_url: &str,
        api_key: Option<&str>,
        target_index: &str,
        temp_index: &str,
    ) -> Result<()> {
        let client = Client::new(meilisearch_url, api_key).map_err(|e| {
            ScrapixError::Storage(format!(
                "Failed to create Meilisearch client for swap: {}",
                e
            ))
        })?;

        // Ensure the target index exists (first crawl case)
        let _ = client.create_index(target_index, Some("uid")).await;

        // Wait for all pending indexing tasks on the temp index to complete
        Self::wait_for_index_idle_with_client(&client, temp_index, Duration::from_secs(300))
            .await?;

        info!(
            target = %target_index,
            temp = %temp_index,
            "All indexing tasks settled, performing atomic swap"
        );

        // Perform the atomic swap
        let swap = SwapIndexes {
            indexes: (target_index.to_string(), temp_index.to_string()),
        };
        let task_info = client.swap_indexes([&swap]).await.map_err(|e| {
            ScrapixError::Storage(format!(
                "Failed to swap indexes {} <-> {}: {}",
                target_index, temp_index, e
            ))
        })?;

        // Wait for swap to complete
        task_info
            .wait_for_completion(
                &client,
                Some(Duration::from_millis(200)),
                Some(Duration::from_secs(60)),
            )
            .await
            .map_err(|e| ScrapixError::Storage(format!("Swap task failed: {}", e)))?;

        info!(
            target = %target_index,
            temp = %temp_index,
            "Index swap completed successfully"
        );

        // Delete the temp index (which now holds old data)
        if let Err(e) = Self::delete_index_with_client(&client, temp_index).await {
            warn!(
                temp = %temp_index,
                error = %e,
                "Failed to delete temp index after swap (non-fatal)"
            );
        }

        Ok(())
    }

    /// Delete a temp index (best-effort cleanup, e.g. on job failure).
    pub async fn cleanup_temp_index(
        meilisearch_url: &str,
        api_key: Option<&str>,
        index_uid: &str,
    ) -> Result<()> {
        let client = Client::new(meilisearch_url, api_key).map_err(|e| {
            ScrapixError::Storage(format!(
                "Failed to create Meilisearch client for cleanup: {}",
                e
            ))
        })?;

        Self::delete_index_with_client(&client, index_uid).await
    }

    /// Delete an index via the given client.
    async fn delete_index_with_client(client: &Client, index_uid: &str) -> Result<()> {
        let task_info = client.index(index_uid).delete().await.map_err(|e| {
            ScrapixError::Storage(format!("Failed to delete index {}: {}", index_uid, e))
        })?;

        task_info
            .wait_for_completion(
                client,
                Some(Duration::from_millis(200)),
                Some(Duration::from_secs(30)),
            )
            .await
            .map_err(|e| {
                ScrapixError::Storage(format!("Delete index task failed for {}: {}", index_uid, e))
            })?;

        info!(index = %index_uid, "Index deleted");
        Ok(())
    }

    /// Wait until all tasks for a specific index are finished (no enqueued/processing tasks).
    async fn wait_for_index_idle_with_client(
        client: &Client,
        index_uid: &str,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            if tokio::time::Instant::now() > deadline {
                return Err(ScrapixError::Storage(format!(
                    "Timed out waiting for index {} tasks to settle ({}s)",
                    index_uid,
                    timeout.as_secs()
                )));
            }

            let mut query = TasksSearchQuery::new(client);
            query
                .with_index_uids([index_uid])
                .with_statuses(["enqueued", "processing"]);

            let result = client.get_tasks_with(&query).await.map_err(|e| {
                ScrapixError::Storage(format!(
                    "Failed to query tasks for index {}: {}",
                    index_uid, e
                ))
            })?;

            let pending_count = result.results.len();
            if pending_count == 0 {
                return Ok(());
            }

            debug!(
                index = %index_uid,
                pending = pending_count,
                "Waiting for index tasks to settle"
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

/// Implementation of core Storage trait
#[async_trait]
impl scrapix_core::traits::Storage for MeilisearchStorage {
    async fn add(&self, doc: Document) -> Result<()> {
        self.add_document(doc).await
    }

    async fn add_batch(&self, docs: Vec<Document>) -> Result<()> {
        self.add_documents(docs).await
    }

    async fn flush(&self) -> Result<usize> {
        MeilisearchStorage::flush(self).await
    }

    async fn count(&self) -> Result<u64> {
        MeilisearchStorage::count(self).await
    }
}

/// Builder for MeilisearchStorage
pub struct MeilisearchStorageBuilder {
    config: MeilisearchConfig,
}

impl MeilisearchStorageBuilder {
    pub fn new(url: impl Into<String>, index_uid: impl Into<String>) -> Self {
        Self {
            config: MeilisearchConfig {
                url: url.into(),
                index_uid: index_uid.into(),
                ..Default::default()
            },
        }
    }

    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.config.api_key = Some(key.into());
        self
    }

    pub fn primary_key(mut self, key: impl Into<String>) -> Self {
        self.config.primary_key = key.into();
        self
    }

    pub fn searchable_attributes(mut self, attrs: Vec<String>) -> Self {
        self.config.searchable_attributes = attrs;
        self
    }

    pub fn filterable_attributes(mut self, attrs: Vec<String>) -> Self {
        self.config.filterable_attributes = attrs;
        self
    }

    pub fn sortable_attributes(mut self, attrs: Vec<String>) -> Self {
        self.config.sortable_attributes = attrs;
        self
    }

    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    pub async fn build(self) -> Result<MeilisearchStorage> {
        MeilisearchStorage::new(self.config).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = MeilisearchConfig::default();
        assert_eq!(config.url, "http://localhost:7700");
        assert_eq!(config.index_uid, "documents");
        assert_eq!(config.primary_key, "uid");
        assert_eq!(config.batch_size, 1000);
    }

    #[test]
    fn test_builder() {
        let builder = MeilisearchStorageBuilder::new("http://localhost:7700", "test_index")
            .api_key("my_key")
            .batch_size(500);

        assert_eq!(builder.config.url, "http://localhost:7700");
        assert_eq!(builder.config.index_uid, "test_index");
        assert_eq!(builder.config.api_key, Some("my_key".to_string()));
        assert_eq!(builder.config.batch_size, 500);
    }
}
