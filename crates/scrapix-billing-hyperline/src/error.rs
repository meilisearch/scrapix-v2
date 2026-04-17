use thiserror::Error;

#[derive(Debug, Error)]
pub enum HyperlineError {
    #[error("missing environment variable: {0}")]
    MissingEnv(&'static str),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("http transport error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("hyperline api error: {status} {message}")]
    Api { status: u16, message: String },

    #[error("decode error: {0}")]
    Decode(#[from] serde_json::Error),

    #[error("invalid webhook signature")]
    InvalidSignature,

    #[error("webhook timestamp outside 5-minute tolerance")]
    StaleTimestamp,

    #[error("malformed webhook header: {0}")]
    MalformedHeader(&'static str),
}
