//! Scrapix Auth
//!
//! Authentication primitives extracted from the API server so they can be
//! reused by workers, CLI tools, and the MCP server.

pub mod jwt;
pub mod password;
pub mod rate_limit;
pub mod types;

pub use jwt::{decode_jwt, encode_jwt, Claims};
pub use password::{hash_password, verify_password};
pub use rate_limit::InMemoryAuthRateLimiter;
pub use types::{AuthenticatedAccount, AuthenticatedUser};
