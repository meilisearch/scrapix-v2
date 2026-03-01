//! Message bus abstraction traits and concrete enum wrappers.
//!
//! These traits allow swapping between Kafka (distributed) and in-process channels
//! (single-binary mode) without changing service code.
//!
//! Because the trait methods are generic (they take `T: Serialize` / `T: DeserializeOwned`),
//! the traits are **not** object-safe and cannot be used as `dyn MessageProducer`.
//! Use the concrete enum wrappers [`AnyProducer`] and [`AnyConsumer`] instead — they
//! wrap either variant behind an `Arc` and delegate to the right implementation.

use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::de::DeserializeOwned;
use serde::Serialize;

use scrapix_core::Result;

use crate::channel::{ChannelConsumer, ChannelProducer};
use crate::consumer::KafkaConsumer;
use crate::producer::KafkaProducer;
use crate::MessageMetadata;

/// Trait for sending messages to topics.
///
/// Implemented by `KafkaProducer` (distributed) and `ChannelProducer` (in-process).
///
/// **Not object-safe** — use [`AnyProducer`] for storage/passing around.
#[async_trait]
pub trait MessageProducer: Send + Sync + 'static {
    /// Send a serializable message to a topic with an optional partition key.
    async fn send<T: Serialize + Send + Sync>(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &T,
    ) -> Result<(i32, i64)>;

    /// Send raw bytes to a topic.
    async fn send_raw(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &[u8],
    ) -> Result<(i32, i64)>;

    /// Flush all pending messages.
    fn flush(&self, timeout: Duration);

    /// Check if the producer is connected and healthy.
    fn is_healthy(&self) -> bool;
}

/// Trait for consuming messages from topics.
///
/// Implemented by `KafkaConsumer` (distributed) and `ChannelConsumer` (in-process).
///
/// **Not object-safe** — use [`AnyConsumer`] for storage/passing around.
#[async_trait]
pub trait MessageConsumer: Send + Sync + 'static {
    /// Subscribe to one or more topics.
    fn subscribe(&self, topics: &[&str]) -> Result<()>;

    /// Process messages sequentially with a handler function.
    async fn process<T, F, Fut>(&self, handler: F) -> Result<()>
    where
        T: DeserializeOwned + Send + 'static,
        F: FnMut(T, MessageMetadata) -> Fut + Send,
        Fut: Future<Output = Result<()>> + Send;

    /// Process messages concurrently, spawning up to `concurrency` tasks.
    async fn process_concurrent<T, F, Fut>(
        &self,
        handler: F,
        concurrency: usize,
        shutdown: Arc<AtomicBool>,
    ) -> Result<()>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(T, MessageMetadata) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static;

    /// Poll for a single message with timeout.
    async fn poll_one<T: DeserializeOwned + Send>(&self, timeout: Duration) -> Result<Option<T>>;
}

// ============================================================================
// Concrete enum wrappers
// ============================================================================

/// Concrete producer that is either a Kafka producer or an in-process channel producer.
///
/// Use this instead of `Arc<dyn MessageProducer>` — the trait is not object-safe due to
/// generic methods (`send<T>`, etc.).
#[derive(Clone)]
pub enum AnyProducer {
    Kafka(Arc<KafkaProducer>),
    Channel(Arc<ChannelProducer>),
}

impl AnyProducer {
    /// Wrap a `KafkaProducer`.
    pub fn kafka(p: KafkaProducer) -> Self {
        Self::Kafka(Arc::new(p))
    }

    /// Wrap a `ChannelProducer`.
    pub fn channel(p: ChannelProducer) -> Self {
        Self::Channel(Arc::new(p))
    }

    /// Send a serializable message to a topic.
    pub async fn send<T: Serialize + Send + Sync>(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &T,
    ) -> Result<(i32, i64)> {
        match self {
            Self::Kafka(p) => p.send(topic, key, payload).await,
            Self::Channel(p) => p.send(topic, key, payload).await,
        }
    }

    /// Send raw bytes to a topic.
    pub async fn send_raw(
        &self,
        topic: &str,
        key: Option<&str>,
        payload: &[u8],
    ) -> Result<(i32, i64)> {
        match self {
            Self::Kafka(p) => p.send_raw(topic, key, payload, None).await,
            Self::Channel(p) => p.send_raw(topic, key, payload).await,
        }
    }

    /// Flush all pending messages.
    pub fn flush(&self, timeout: Duration) {
        match self {
            Self::Kafka(p) => p.flush(timeout),
            Self::Channel(p) => p.flush(timeout),
        }
    }

    /// Check if the producer is healthy.
    pub fn is_healthy(&self) -> bool {
        match self {
            Self::Kafka(p) => p.is_healthy(),
            Self::Channel(p) => p.is_healthy(),
        }
    }
}

impl From<KafkaProducer> for AnyProducer {
    fn from(p: KafkaProducer) -> Self {
        Self::kafka(p)
    }
}

impl From<ChannelProducer> for AnyProducer {
    fn from(p: ChannelProducer) -> Self {
        Self::channel(p)
    }
}

/// Concrete consumer that is either a Kafka consumer or an in-process channel consumer.
///
/// Use this instead of `Arc<dyn MessageConsumer>` — the trait is not object-safe due to
/// generic methods (`poll_one<T>`, `process<T>`, etc.).
#[derive(Clone)]
pub enum AnyConsumer {
    Kafka(Arc<KafkaConsumer>),
    Channel(Arc<ChannelConsumer>),
}

impl AnyConsumer {
    /// Wrap a `KafkaConsumer`.
    pub fn kafka(c: KafkaConsumer) -> Self {
        Self::Kafka(Arc::new(c))
    }

    /// Wrap a `ChannelConsumer`.
    pub fn channel(c: ChannelConsumer) -> Self {
        Self::Channel(Arc::new(c))
    }

    /// Subscribe to topics.
    pub fn subscribe(&self, topics: &[&str]) -> Result<()> {
        match self {
            Self::Kafka(c) => c.subscribe(topics),
            Self::Channel(c) => c.subscribe(topics),
        }
    }

    /// Process messages sequentially with a handler function.
    pub async fn process<T, F, Fut>(&self, handler: F) -> Result<()>
    where
        T: DeserializeOwned + Send + 'static,
        F: FnMut(T, MessageMetadata) -> Fut + Send,
        Fut: Future<Output = Result<()>> + Send,
    {
        match self {
            Self::Kafka(c) => c.process(handler).await,
            Self::Channel(c) => c.process(handler).await,
        }
    }

    /// Process messages concurrently, spawning up to `concurrency` tasks.
    pub async fn process_concurrent<T, F, Fut>(
        &self,
        handler: F,
        concurrency: usize,
        shutdown: Arc<AtomicBool>,
    ) -> Result<()>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(T, MessageMetadata) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        match self {
            Self::Kafka(c) => c.process_concurrent(handler, concurrency, shutdown).await,
            Self::Channel(c) => c.process_concurrent(handler, concurrency, shutdown).await,
        }
    }

    /// Poll for a single message with timeout.
    pub async fn poll_one<T: DeserializeOwned + Send>(
        &self,
        timeout: Duration,
    ) -> Result<Option<T>> {
        match self {
            Self::Kafka(c) => c.poll_one(timeout).await,
            Self::Channel(c) => c.poll_one(timeout).await,
        }
    }
}

impl From<KafkaConsumer> for AnyConsumer {
    fn from(c: KafkaConsumer) -> Self {
        Self::kafka(c)
    }
}

impl From<ChannelConsumer> for AnyConsumer {
    fn from(c: ChannelConsumer) -> Self {
        Self::channel(c)
    }
}
