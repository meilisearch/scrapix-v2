//! # Scrapix Core
//!
//! Core types, traits, and utilities for the Scrapix web crawler.
//!
//! This crate provides the foundational components used across all Scrapix services:
//! - Configuration schemas
//! - Document types
//! - Error types
//! - Core traits
//! - Billing types (accounts, API keys, usage tracking)

pub mod billing;
pub mod config;
pub mod document;
pub mod error;
pub mod traits;

pub use billing::*;
pub use config::*;
pub use document::*;
pub use error::*;
