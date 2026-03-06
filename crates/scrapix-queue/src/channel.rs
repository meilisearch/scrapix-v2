//! In-process message bus using bounded async channels.
//!
//! This replaces Kafka when running all services in a single process (`scrapix all`).
//! Uses `async-channel` (mpmc, bounded) to simulate topics.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tracing::{debug, error};

use scrapix_core::{Result, ScrapixError};

use crate::traits::{MessageConsumer, MessageProducer};
use crate::MessageMetadata;

/// Default channel capacity per topic.
const DEFAULT_CAPACITY: usize = 50_000;

/// In-process message bus that holds topic channels.
///
/// Create one `ChannelBus`, then call `producer()` and `consumer()` to get
/// handles that implement `MessageProducer` / `MessageConsumer`.
pub struct ChannelBus {
    topics: Arc<RwLock<HashMap<String, TopicChannel>>>,
    capacity: usize,
}

struct TopicChannel {
    sender: Sender<Vec<u8>>,
    receiver: Receiver<Vec<u8>>,
    offset: AtomicI64,
}

impl ChannelBus {
    /// Create a new channel bus with default capacity (50,000 messages per topic).
    pub fn new() -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            capacity: DEFAULT_CAPACITY,
        }
    }

    /// Create a new channel bus with custom capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            topics: Arc::new(RwLock::new(HashMap::new())),
            capacity,
        }
    }

    /// Create a producer handle.
    pub fn producer(&self) -> ChannelProducer {
        ChannelProducer {
            bus: self.topics.clone(),
            capacity: self.capacity,
        }
    }

    /// Create a consumer handle.
    pub fn consumer(&self) -> ChannelConsumer {
        ChannelConsumer {
            bus: self.topics.clone(),
            capacity: self.capacity,
            subscriptions: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

impl Default for ChannelBus {
    fn default() -> Self {
        Self::new()
    }
}

/// In-process message producer.
pub struct ChannelProducer {
    bus: Arc<RwLock<HashMap<String, TopicChannel>>>,
    capacity: usize,
}

impl ChannelProducer {
    fn get_or_create_topic(&self, topic: &str) -> (Sender<Vec<u8>>, i64) {
        // Fast path
        {
            let topics = self.bus.read();
            if let Some(tc) = topics.get(topic) {
                let offset = tc.offset.fetch_add(1, Ordering::Relaxed);
                return (tc.sender.clone(), offset);
            }
        }

        // Slow path
        let mut topics = self.bus.write();
        let tc = topics.entry(topic.to_string()).or_insert_with(|| {
            let (sender, receiver) = async_channel::bounded(self.capacity);
            debug!(topic = topic, "Created topic channel (from producer)");
            TopicChannel {
                sender,
                receiver,
                offset: AtomicI64::new(0),
            }
        });
        let offset = tc.offset.fetch_add(1, Ordering::Relaxed);
        (tc.sender.clone(), offset)
    }
}

#[async_trait::async_trait]
impl MessageProducer for ChannelProducer {
    async fn send<T: Serialize + Send + Sync>(
        &self,
        topic: &str,
        _key: Option<&str>,
        payload: &T,
    ) -> Result<(i32, i64)> {
        let bytes = serde_json::to_vec(payload)
            .map_err(|e| ScrapixError::Queue(format!("Serialization failed: {}", e)))?;

        let (sender, offset) = self.get_or_create_topic(topic);
        sender
            .send(bytes)
            .await
            .map_err(|e| ScrapixError::Queue(format!("Channel send failed: {}", e)))?;

        debug!(topic = topic, offset = offset, "Channel message sent");
        Ok((0, offset)) // partition=0 for channels
    }

    async fn send_raw(
        &self,
        topic: &str,
        _key: Option<&str>,
        payload: &[u8],
    ) -> Result<(i32, i64)> {
        let (sender, offset) = self.get_or_create_topic(topic);
        sender
            .send(payload.to_vec())
            .await
            .map_err(|e| ScrapixError::Queue(format!("Channel send failed: {}", e)))?;

        Ok((0, offset))
    }

    fn flush(&self, _timeout: Duration) {
        // No-op for channels — messages are delivered immediately
    }

    fn is_healthy(&self) -> bool {
        true // Always healthy
    }
}

/// In-process message consumer.
pub struct ChannelConsumer {
    bus: Arc<RwLock<HashMap<String, TopicChannel>>>,
    capacity: usize,
    subscriptions: Arc<RwLock<Vec<String>>>,
}

impl ChannelConsumer {
    fn get_receiver(&self, topic: &str) -> Receiver<Vec<u8>> {
        // Fast path
        {
            let topics = self.bus.read();
            if let Some(tc) = topics.get(topic) {
                return tc.receiver.clone();
            }
        }

        // Create topic if it doesn't exist yet
        let mut topics = self.bus.write();
        let tc = topics.entry(topic.to_string()).or_insert_with(|| {
            let (sender, receiver) = async_channel::bounded(self.capacity);
            debug!(topic = topic, "Created topic channel (from consumer)");
            TopicChannel {
                sender,
                receiver,
                offset: AtomicI64::new(0),
            }
        });
        tc.receiver.clone()
    }
}

#[async_trait::async_trait]
impl MessageConsumer for ChannelConsumer {
    fn subscribe(&self, topics: &[&str]) -> Result<()> {
        let mut subs = self.subscriptions.write();
        for topic in topics {
            if !subs.contains(&topic.to_string()) {
                subs.push(topic.to_string());
            }
        }
        debug!(topics = ?topics, "Channel consumer subscribed");
        Ok(())
    }

    async fn process<T, F, Fut>(&self, mut handler: F) -> Result<()>
    where
        T: DeserializeOwned + Send + 'static,
        F: FnMut(T, MessageMetadata) -> Fut + Send,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let topics: Vec<String> = self.subscriptions.read().clone();
        if topics.is_empty() {
            return Err(ScrapixError::Queue("No topics subscribed".into()));
        }

        // For single-topic subscriptions (most common), use direct receive
        let receiver = self.get_receiver(&topics[0]);
        let mut offset = 0i64;

        while let Ok(bytes) = receiver.recv().await {
            let metadata = MessageMetadata {
                topic: topics[0].clone(),
                partition: 0,
                offset,
                key: None,
                timestamp: Some(chrono::Utc::now().timestamp_millis()),
            };
            offset += 1;

            match serde_json::from_slice::<T>(&bytes) {
                Ok(payload) => {
                    if let Err(e) = handler(payload, metadata.clone()).await {
                        error!(
                            topic = %metadata.topic,
                            offset = metadata.offset,
                            error = %e,
                            "Handler error"
                        );
                    }
                }
                Err(e) => {
                    error!(
                        topic = %metadata.topic,
                        offset = metadata.offset,
                        error = %e,
                        "Deserialization error"
                    );
                }
            }
        }

        Ok(())
    }

    async fn process_concurrent<T, F, Fut>(
        &self,
        handler: F,
        concurrency: usize,
        shutdown: Arc<AtomicBool>,
    ) -> Result<()>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(T, MessageMetadata) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let topics: Vec<String> = self.subscriptions.read().clone();
        if topics.is_empty() {
            return Err(ScrapixError::Queue("No topics subscribed".into()));
        }

        let receiver = self.get_receiver(&topics[0]);
        let topic_name = topics[0].clone();
        let handler = Arc::new(handler);
        let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));
        let mut offset = 0i64;

        loop {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }

            match tokio::time::timeout(Duration::from_millis(100), receiver.recv()).await {
                Ok(Ok(bytes)) => {
                    let metadata = MessageMetadata {
                        topic: topic_name.clone(),
                        partition: 0,
                        offset,
                        key: None,
                        timestamp: Some(chrono::Utc::now().timestamp_millis()),
                    };
                    offset += 1;

                    match serde_json::from_slice::<T>(&bytes) {
                        Ok(payload) => {
                            let permit = match semaphore.clone().acquire_owned().await {
                                Ok(permit) => permit,
                                Err(_) => break,
                            };

                            let handler = handler.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handler(payload, metadata.clone()).await {
                                    error!(
                                        topic = %metadata.topic,
                                        offset = metadata.offset,
                                        error = %e,
                                        "Handler error"
                                    );
                                }
                                drop(permit);
                            });
                        }
                        Err(e) => {
                            error!(
                                topic = %metadata.topic,
                                offset = metadata.offset,
                                error = %e,
                                "Deserialization error"
                            );
                        }
                    }
                }
                Ok(Err(_)) => {
                    // Channel closed
                    break;
                }
                Err(_) => {
                    // Timeout — check shutdown and continue
                }
            }
        }

        // Wait for all in-flight tasks
        let _ = semaphore.acquire_many(concurrency as u32).await;
        Ok(())
    }

    async fn poll_one<T: DeserializeOwned + Send>(&self, timeout: Duration) -> Result<Option<T>> {
        let topics: Vec<String> = self.subscriptions.read().clone();
        if topics.is_empty() {
            return Err(ScrapixError::Queue("No topics subscribed".into()));
        }

        let receiver = self.get_receiver(&topics[0]);

        match tokio::time::timeout(timeout, receiver.recv()).await {
            Ok(Ok(bytes)) => {
                let payload = serde_json::from_slice::<T>(&bytes)
                    .map_err(|e| ScrapixError::Queue(format!("Deserialization failed: {}", e)))?;
                Ok(Some(payload))
            }
            Ok(Err(_)) => Ok(None), // Channel closed
            Err(_) => Ok(None),     // Timeout
        }
    }
}
