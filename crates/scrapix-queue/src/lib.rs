//! # Scrapix Queue
//!
//! Message queue integration for Redpanda/Kafka.
//!
//! ## Features
//!
//! - URL frontier queue for distributing crawl work
//! - Crawl event streaming for monitoring
//! - Content processing queue for document extraction
//! - Dead letter queue handling for failed messages
//!
//! ## Topics
//!
//! - `scrapix.urls.frontier` - URLs waiting to be crawled
//! - `scrapix.urls.processing` - URLs currently being processed
//! - `scrapix.pages.raw` - Raw crawled pages
//! - `scrapix.documents` - Processed documents for indexing
//! - `scrapix.dlq.urls` - Dead letter queue for failed URLs
//! - `scrapix.events` - Crawl events for monitoring
//!
//! ## Example
//!
//! ```rust,ignore
//! use scrapix_queue::{ProducerBuilder, ConsumerBuilder, topics};
//! use scrapix_core::CrawlUrl;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // Create a producer
//!     let producer = ProducerBuilder::new("localhost:9092")
//!         .client_id("crawler-producer")
//!         .build()?;
//!
//!     // Send a URL to the frontier
//!     let url_msg = topics::UrlMessage::new(
//!         CrawlUrl::seed("https://example.com"),
//!         "job-123",
//!         "my_index"
//!     );
//!     producer.send(topics::names::URL_FRONTIER, Some(&url_msg.job_id), &url_msg).await?;
//!
//!     // Create a consumer
//!     let consumer = ConsumerBuilder::new("localhost:9092", "crawler-group")
//!         .auto_offset_reset("earliest")
//!         .build()?;
//!
//!     consumer.subscribe(&[topics::names::URL_FRONTIER])?;
//!
//!     Ok(())
//! }
//! ```

pub mod bus;
pub mod channel;
pub mod consumer;
pub mod producer;
pub mod topics;
pub mod traits;

// Re-exports
pub use consumer::{ConsumerBuilder, ConsumerConfig, KafkaConsumer, MessageMetadata};
pub use producer::{KafkaProducer, ProducerBuilder, ProducerConfig};
pub use topics::{
    names as topic_names, CrawlEvent, CrawlHistoryMessage, DlqMessage, DocumentMessage,
    LinksMessage, RawPageMessage, UrlMessage,
};

// Message bus abstractions
pub use channel::{ChannelBus, ChannelConsumer, ChannelProducer};
pub use traits::{AnyConsumer, AnyProducer, MessageConsumer, MessageProducer};
