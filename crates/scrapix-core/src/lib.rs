//! # Scrapix Core
//!
//! Core types, traits, and utilities for the Scrapix web crawler.
//!
//! This crate provides the foundational components used across all Scrapix services:
//! - Configuration schemas
//! - Document types
//! - Error types
//! - Core traits

pub mod config;
pub mod document;
pub mod error;
pub mod traits;

pub use config::*;
pub use document::*;
pub use error::*;
