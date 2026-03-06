//! Kafka/Redpanda message producer

use std::time::Duration;

use rdkafka::{
    config::ClientConfig,
    message::{Header, OwnedHeaders},
    producer::{FutureProducer, FutureRecord, Producer},
    util::Timeout,
};
use serde::Serialize;
use tracing::{debug, error, instrument};

use scrapix_core::{Result, ScrapixError};

/// Producer configuration
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    /// Kafka/Redpanda broker addresses
    pub brokers: String,
    /// Client ID
    pub client_id: String,
    /// Message timeout
    pub message_timeout: Duration,
    /// Request timeout
    pub request_timeout: Duration,
    /// Enable idempotent producer
    pub idempotent: bool,
    /// Compression type (none, gzip, snappy, lz4, zstd)
    pub compression: String,
    /// Batch size
    pub batch_size: usize,
    /// Linger time (ms) - how long to wait before sending a batch
    pub linger_ms: u64,
    /// Acks required (0, 1, all)
    pub acks: String,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            brokers: "localhost:9092".to_string(),
            client_id: "scrapix-producer".to_string(),
            message_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(5),
            idempotent: true,
            compression: "lz4".to_string(),
            batch_size: 65536,
            linger_ms: 20,
            acks: "all".to_string(),
        }
    }
}

/// Kafka/Redpanda message producer
pub struct KafkaProducer {
    producer: FutureProducer,
    config: ProducerConfig,
}

impl KafkaProducer {
    /// Create a new Kafka producer
    pub fn new(config: ProducerConfig) -> Result<Self> {
        let mut client_config = ClientConfig::new();

        client_config
            .set("bootstrap.servers", &config.brokers)
            .set("client.id", &config.client_id)
            .set(
                "message.timeout.ms",
                config.message_timeout.as_millis().to_string(),
            )
            .set(
                "request.timeout.ms",
                config.request_timeout.as_millis().to_string(),
            )
            .set("compression.type", &config.compression)
            .set("batch.size", config.batch_size.to_string())
            .set("linger.ms", config.linger_ms.to_string())
            .set("acks", &config.acks);

        if config.idempotent {
            client_config.set("enable.idempotence", "true");
        }

        let producer: FutureProducer = client_config
            .create()
            .map_err(|e| ScrapixError::Queue(format!("Failed to create producer: {}", e)))?;

        Ok(Self { producer, config })
    }

    /// Create a producer with default configuration
    pub fn with_brokers(brokers: impl Into<String>) -> Result<Self> {
        Self::new(ProducerConfig {
            brokers: brokers.into(),
            ..Default::default()
        })
    }

    /// Send a message to a topic
    #[instrument(skip(self, payload), fields(topic = %topic))]
    pub async fn send<T: Serialize>(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &T,
    ) -> Result<(i32, i64)> {
        let payload_bytes = serde_json::to_vec(payload)
            .map_err(|e| ScrapixError::Queue(format!("Serialization failed: {}", e)))?;

        self.send_raw(topic, key, &payload_bytes, None).await
    }

    /// Send a message with custom headers
    pub async fn send_with_headers<T: Serialize>(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &T,
        headers: Vec<(&str, &str)>,
    ) -> Result<(i32, i64)> {
        let payload_bytes = serde_json::to_vec(payload)
            .map_err(|e| ScrapixError::Queue(format!("Serialization failed: {}", e)))?;

        self.send_raw(topic, key, &payload_bytes, Some(headers))
            .await
    }

    /// Send raw bytes to a topic
    pub async fn send_raw(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &[u8],
        headers: Option<Vec<(&str, &str)>>,
    ) -> Result<(i32, i64)> {
        let mut record = FutureRecord::to(topic).payload(payload);

        if let Some(k) = key {
            record = record.key(k);
        }

        let owned_headers = if let Some(hdrs) = headers {
            let mut h = OwnedHeaders::new();
            for (name, value) in hdrs {
                h = h.insert(Header {
                    key: name,
                    value: Some(value),
                });
            }
            Some(h)
        } else {
            None
        };

        if let Some(h) = owned_headers {
            record = record.headers(h);
        }

        let timeout = Timeout::After(self.config.message_timeout);

        match self.producer.send(record, timeout).await {
            Ok(delivery) => {
                let partition = delivery.partition;
                let offset = delivery.offset;
                debug!(topic, partition, offset, "Message sent");
                Ok((partition, offset))
            }
            Err((err, _)) => {
                error!(topic, error = %err, "Failed to send message");
                Err(ScrapixError::Queue(format!(
                    "Failed to send message: {}",
                    err
                )))
            }
        }
    }

    /// Send multiple messages to a topic (batched)
    pub async fn send_batch<T: Serialize>(
        &self,
        topic: &str,
        messages: Vec<(Option<String>, T)>,
    ) -> Result<Vec<std::result::Result<(i32, i64), String>>> {
        let mut results = Vec::with_capacity(messages.len());

        for (key, payload) in messages {
            let result = match self.send(topic, key.as_deref(), &payload).await {
                Ok(r) => Ok(r),
                Err(e) => Err(e.to_string()),
            };
            results.push(result);
        }

        Ok(results)
    }

    /// Flush all pending messages
    pub fn flush(&self, timeout: Duration) {
        let _ = self.producer.flush(Timeout::After(timeout));
    }

    /// Check if the producer is connected and can reach the broker
    pub fn is_healthy(&self) -> bool {
        // Try to get cluster metadata with a short timeout
        // This verifies we can communicate with the broker
        match self
            .producer
            .client()
            .fetch_metadata(None, Timeout::After(Duration::from_secs(5)))
        {
            Ok(metadata) => {
                // Check that we have at least one broker
                !metadata.brokers().is_empty()
            }
            Err(_) => false,
        }
    }

    /// Get broker information for diagnostics
    pub fn broker_count(&self) -> Option<usize> {
        self.producer
            .client()
            .fetch_metadata(None, Timeout::After(Duration::from_secs(5)))
            .ok()
            .map(|m| m.brokers().len())
    }
}

/// Builder for KafkaProducer
pub struct ProducerBuilder {
    config: ProducerConfig,
}

impl ProducerBuilder {
    pub fn new(brokers: impl Into<String>) -> Self {
        Self {
            config: ProducerConfig {
                brokers: brokers.into(),
                ..Default::default()
            },
        }
    }

    pub fn client_id(mut self, id: impl Into<String>) -> Self {
        self.config.client_id = id.into();
        self
    }

    pub fn message_timeout(mut self, timeout: Duration) -> Self {
        self.config.message_timeout = timeout;
        self
    }

    pub fn compression(mut self, compression: impl Into<String>) -> Self {
        self.config.compression = compression.into();
        self
    }

    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batch_size = size;
        self
    }

    pub fn linger_ms(mut self, ms: u64) -> Self {
        self.config.linger_ms = ms;
        self
    }

    pub fn acks(mut self, acks: impl Into<String>) -> Self {
        self.config.acks = acks.into();
        self
    }

    pub fn idempotent(mut self, enabled: bool) -> Self {
        self.config.idempotent = enabled;
        self
    }

    pub fn build(self) -> Result<KafkaProducer> {
        KafkaProducer::new(self.config)
    }
}

// Implement the MessageProducer trait for KafkaProducer
#[async_trait::async_trait]
impl crate::traits::MessageProducer for KafkaProducer {
    async fn send<T: Serialize + Send + Sync>(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &T,
    ) -> Result<(i32, i64)> {
        KafkaProducer::send(self, topic, key, payload).await
    }

    async fn send_raw(&self, topic: &str, key: Option<&str>, payload: &[u8]) -> Result<(i32, i64)> {
        KafkaProducer::send_raw(self, topic, key, payload, None).await
    }

    fn flush(&self, timeout: Duration) {
        KafkaProducer::flush(self, timeout)
    }

    fn is_healthy(&self) -> bool {
        KafkaProducer::is_healthy(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_producer_config_default() {
        let config = ProducerConfig::default();
        assert_eq!(config.brokers, "localhost:9092");
        assert_eq!(config.compression, "lz4");
        assert!(config.idempotent);
    }

    #[test]
    fn test_builder() {
        let builder = ProducerBuilder::new("broker1:9092,broker2:9092")
            .client_id("test-producer")
            .compression("gzip")
            .batch_size(32768);

        assert_eq!(builder.config.brokers, "broker1:9092,broker2:9092");
        assert_eq!(builder.config.client_id, "test-producer");
        assert_eq!(builder.config.compression, "gzip");
        assert_eq!(builder.config.batch_size, 32768);
    }
}
