//! Kafka/Redpanda message consumer

use std::time::Duration;

use rdkafka::{
    config::ClientConfig,
    consumer::{CommitMode, Consumer, StreamConsumer},
    message::{BorrowedMessage, Message as KafkaMessage},
    TopicPartitionList,
};
use serde::de::DeserializeOwned;
use tokio_stream::StreamExt;
use tracing::{error, warn};

use scrapix_core::{Result, ScrapixError};

/// Consumer configuration
#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    /// Kafka/Redpanda broker addresses
    pub brokers: String,
    /// Consumer group ID
    pub group_id: String,
    /// Client ID
    pub client_id: String,
    /// Auto commit interval
    pub auto_commit_interval: Duration,
    /// Session timeout
    pub session_timeout: Duration,
    /// Enable auto commit
    pub enable_auto_commit: bool,
    /// Auto offset reset (earliest, latest)
    pub auto_offset_reset: String,
    /// Max poll interval
    pub max_poll_interval: Duration,
    /// Fetch min bytes
    pub fetch_min_bytes: i32,
    /// Fetch max wait ms
    pub fetch_max_wait_ms: i32,
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            brokers: "localhost:9092".to_string(),
            group_id: "scrapix-consumer".to_string(),
            client_id: "scrapix-consumer".to_string(),
            auto_commit_interval: Duration::from_secs(5),
            session_timeout: Duration::from_secs(30),
            enable_auto_commit: false, // Manual commit for reliability
            auto_offset_reset: "earliest".to_string(),
            max_poll_interval: Duration::from_secs(300),
            fetch_min_bytes: 1,
            fetch_max_wait_ms: 500,
        }
    }
}

/// Kafka/Redpanda message consumer
pub struct KafkaConsumer {
    consumer: StreamConsumer,
    #[allow(dead_code)]
    config: ConsumerConfig,
}

impl KafkaConsumer {
    /// Create a new Kafka consumer
    pub fn new(config: ConsumerConfig) -> Result<Self> {
        let mut client_config = ClientConfig::new();

        client_config
            .set("bootstrap.servers", &config.brokers)
            .set("group.id", &config.group_id)
            .set("client.id", &config.client_id)
            .set("enable.auto.commit", config.enable_auto_commit.to_string())
            .set(
                "auto.commit.interval.ms",
                config.auto_commit_interval.as_millis().to_string(),
            )
            .set(
                "session.timeout.ms",
                config.session_timeout.as_millis().to_string(),
            )
            .set("auto.offset.reset", &config.auto_offset_reset)
            .set(
                "max.poll.interval.ms",
                config.max_poll_interval.as_millis().to_string(),
            )
            .set("fetch.min.bytes", config.fetch_min_bytes.to_string())
            .set("fetch.wait.max.ms", config.fetch_max_wait_ms.to_string());

        let consumer: StreamConsumer = client_config
            .create()
            .map_err(|e| ScrapixError::Queue(format!("Failed to create consumer: {}", e)))?;

        Ok(Self { consumer, config })
    }

    /// Create a consumer with default configuration
    pub fn with_brokers(brokers: impl Into<String>, group_id: impl Into<String>) -> Result<Self> {
        Self::new(ConsumerConfig {
            brokers: brokers.into(),
            group_id: group_id.into(),
            ..Default::default()
        })
    }

    /// Subscribe to topics
    pub fn subscribe(&self, topics: &[&str]) -> Result<()> {
        self.consumer
            .subscribe(topics)
            .map_err(|e| ScrapixError::Queue(format!("Failed to subscribe: {}", e)))
    }

    /// Unsubscribe from all topics
    pub fn unsubscribe(&self) {
        self.consumer.unsubscribe();
    }

    /// Process messages with a handler function
    pub async fn process<T, F, Fut>(&self, mut handler: F) -> Result<()>
    where
        T: DeserializeOwned,
        F: FnMut(T, MessageMetadata) -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut stream = self.consumer.stream();

        while let Some(result) = stream.next().await {
            match result {
                Ok(msg) => {
                    let metadata = MessageMetadata::from_message(&msg);

                    match self.deserialize_message::<T>(&msg) {
                        Ok(payload) => {
                            if let Err(e) = handler(payload, metadata.clone()).await {
                                error!(
                                    topic = %metadata.topic,
                                    partition = metadata.partition,
                                    offset = metadata.offset,
                                    error = %e,
                                    "Handler error"
                                );
                            }

                            // Commit offset after processing
                            if let Err(e) = self.commit_message(&msg) {
                                warn!(error = %e, "Failed to commit offset");
                            }
                        }
                        Err(e) => {
                            error!(
                                topic = %metadata.topic,
                                partition = metadata.partition,
                                offset = metadata.offset,
                                error = %e,
                                "Deserialization error"
                            );
                            // Still commit to avoid reprocessing bad messages
                            let _ = self.commit_message(&msg);
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, "Kafka error");
                }
            }
        }

        Ok(())
    }

    /// Receive a batch of messages
    pub async fn receive_batch<T: DeserializeOwned>(
        &self,
        max_messages: usize,
        timeout: Duration,
    ) -> Result<Vec<(T, MessageMetadata)>> {
        let mut messages = Vec::with_capacity(max_messages);
        let deadline = tokio::time::Instant::now() + timeout;
        let mut stream = self.consumer.stream();

        while messages.len() < max_messages && tokio::time::Instant::now() < deadline {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(Ok(msg))) => {
                    let metadata = MessageMetadata::from_message(&msg);
                    if let Ok(payload) = self.deserialize_message::<T>(&msg) {
                        messages.push((payload, metadata));
                    }
                }
                Ok(Some(Err(e))) => {
                    warn!(error = %e, "Kafka error during batch receive");
                }
                Ok(None) => break,
                Err(_) => break, // Timeout
            }
        }

        Ok(messages)
    }

    /// Commit a message's offset
    fn commit_message(&self, msg: &BorrowedMessage<'_>) -> Result<()> {
        self.consumer
            .commit_message(msg, CommitMode::Async)
            .map_err(|e| ScrapixError::Queue(format!("Commit failed: {}", e)))
    }

    /// Commit all consumed offsets
    pub fn commit(&self) -> Result<()> {
        self.consumer
            .commit_consumer_state(CommitMode::Sync)
            .map_err(|e| ScrapixError::Queue(format!("Commit failed: {}", e)))
    }

    /// Deserialize a message payload
    fn deserialize_message<T: DeserializeOwned>(&self, msg: &BorrowedMessage<'_>) -> Result<T> {
        let payload = msg
            .payload()
            .ok_or_else(|| ScrapixError::Queue("Empty message payload".to_string()))?;

        serde_json::from_slice(payload)
            .map_err(|e| ScrapixError::Queue(format!("Deserialization failed: {}", e)))
    }

    /// Get current positions for assigned partitions
    pub fn positions(&self) -> Result<Vec<(String, i32, i64)>> {
        let _assignment = self
            .consumer
            .assignment()
            .map_err(|e| ScrapixError::Queue(format!("Failed to get assignment: {}", e)))?;

        let positions = self
            .consumer
            .position()
            .map_err(|e| ScrapixError::Queue(format!("Failed to get positions: {}", e)))?;

        let mut result = Vec::new();
        for elem in positions.elements() {
            if let Some(offset) = elem.offset().to_raw() {
                result.push((elem.topic().to_string(), elem.partition(), offset));
            }
        }

        Ok(result)
    }

    /// Seek to a specific offset
    pub fn seek(&self, topic: &str, partition: i32, offset: i64) -> Result<()> {
        let mut tpl = TopicPartitionList::new();
        tpl.add_partition_offset(topic, partition, rdkafka::Offset::Offset(offset))
            .map_err(|e| ScrapixError::Queue(format!("Failed to set offset: {}", e)))?;

        self.consumer
            .seek_partitions(tpl, Duration::from_secs(5))
            .map_err(|e| ScrapixError::Queue(format!("Seek failed: {}", e)))?;

        Ok(())
    }

    /// Pause consumption on specific partitions
    pub fn pause(&self, topic: &str, partitions: &[i32]) -> Result<()> {
        let mut tpl = TopicPartitionList::new();
        for &p in partitions {
            tpl.add_partition(topic, p);
        }

        self.consumer
            .pause(&tpl)
            .map_err(|e| ScrapixError::Queue(format!("Pause failed: {}", e)))
    }

    /// Resume consumption on specific partitions
    pub fn resume(&self, topic: &str, partitions: &[i32]) -> Result<()> {
        let mut tpl = TopicPartitionList::new();
        for &p in partitions {
            tpl.add_partition(topic, p);
        }

        self.consumer
            .resume(&tpl)
            .map_err(|e| ScrapixError::Queue(format!("Resume failed: {}", e)))
    }

    /// Get the broker addresses
    pub fn brokers(&self) -> &str {
        &self.config.brokers
    }

    /// Get the consumer group ID
    pub fn group_id(&self) -> &str {
        &self.config.group_id
    }

    /// Poll for a single message with timeout
    pub async fn poll_one<T: DeserializeOwned>(&self, timeout: Duration) -> Result<Option<T>> {
        let mut stream = self.consumer.stream();

        match tokio::time::timeout(timeout, stream.next()).await {
            Ok(Some(Ok(msg))) => {
                let result = self.deserialize_message::<T>(&msg)?;
                // Commit the message
                let _ = self.commit_message(&msg);
                Ok(Some(result))
            }
            Ok(Some(Err(e))) => Err(ScrapixError::Queue(format!("Kafka error: {}", e))),
            Ok(None) => Ok(None),
            Err(_) => Ok(None), // Timeout
        }
    }
}

/// Metadata about a consumed message
#[derive(Debug, Clone)]
pub struct MessageMetadata {
    /// Topic name
    pub topic: String,
    /// Partition number
    pub partition: i32,
    /// Message offset
    pub offset: i64,
    /// Message key (if present)
    pub key: Option<String>,
    /// Message timestamp (milliseconds)
    pub timestamp: Option<i64>,
}

impl MessageMetadata {
    fn from_message(msg: &BorrowedMessage<'_>) -> Self {
        Self {
            topic: msg.topic().to_string(),
            partition: msg.partition(),
            offset: msg.offset(),
            key: msg.key().map(|k| String::from_utf8_lossy(k).to_string()),
            timestamp: msg.timestamp().to_millis(),
        }
    }
}

/// Builder for KafkaConsumer
pub struct ConsumerBuilder {
    config: ConsumerConfig,
}

impl ConsumerBuilder {
    pub fn new(brokers: impl Into<String>, group_id: impl Into<String>) -> Self {
        Self {
            config: ConsumerConfig {
                brokers: brokers.into(),
                group_id: group_id.into(),
                ..Default::default()
            },
        }
    }

    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.config.client_id = id.into();
        self
    }

    pub fn auto_commit(mut self, enabled: bool) -> Self {
        self.config.enable_auto_commit = enabled;
        self
    }

    pub fn auto_offset_reset(mut self, reset: impl Into<String>) -> Self {
        self.config.auto_offset_reset = reset.into();
        self
    }

    pub fn session_timeout(mut self, timeout: Duration) -> Self {
        self.config.session_timeout = timeout;
        self
    }

    pub fn build(self) -> Result<KafkaConsumer> {
        KafkaConsumer::new(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumer_config_default() {
        let config = ConsumerConfig::default();
        assert_eq!(config.brokers, "localhost:9092");
        assert!(!config.enable_auto_commit);
        assert_eq!(config.auto_offset_reset, "earliest");
    }

    #[test]
    fn test_builder() {
        let builder = ConsumerBuilder::new("broker:9092", "test-group")
            .client_id("test-client")
            .auto_commit(true)
            .auto_offset_reset("latest");

        assert_eq!(builder.config.brokers, "broker:9092");
        assert_eq!(builder.config.group_id, "test-group");
        assert!(builder.config.enable_auto_commit);
        assert_eq!(builder.config.auto_offset_reset, "latest");
    }
}
