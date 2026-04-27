//! Webhook signature verification and event parsing.
//!
//! Hyperline signs webhooks with HMAC-SHA256 over `id.timestamp.body` using a
//! base64-encoded secret that starts with `whsec_`. Headers arrive as
//! `webhook-id`, `webhook-timestamp` (Unix seconds), and `webhook-signature`
//! (space-separated list of `v1,<base64>` signatures — any valid one accepts).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::error::HyperlineError;

type HmacSha256 = Hmac<Sha256>;

const TIMESTAMP_TOLERANCE_SECS: i64 = 300;
const SECRET_PREFIX: &str = "whsec_";

pub struct WebhookHeaders<'a> {
    pub id: &'a str,
    pub timestamp: &'a str,
    pub signature: &'a str,
}

/// Verify a Hyperline webhook delivery. Returns `Ok(())` on a valid signature
/// within the 5-minute timestamp tolerance; otherwise returns the failure
/// reason.
pub fn verify_signature(
    secret: &str,
    headers: WebhookHeaders<'_>,
    body: &[u8],
    now_unix: i64,
) -> Result<(), HyperlineError> {
    let raw = secret
        .strip_prefix(SECRET_PREFIX)
        .ok_or(HyperlineError::InvalidConfig(
            "webhook secret must start with whsec_".into(),
        ))?;
    let key = B64
        .decode(raw)
        .map_err(|_| HyperlineError::InvalidConfig("webhook secret is not valid base64".into()))?;

    let ts: i64 = headers
        .timestamp
        .parse()
        .map_err(|_| HyperlineError::MalformedHeader("webhook-timestamp"))?;
    if (now_unix - ts).abs() > TIMESTAMP_TOLERANCE_SECS {
        return Err(HyperlineError::StaleTimestamp);
    }

    // Canonical signed string per Hyperline / Svix spec.
    let mut mac = HmacSha256::new_from_slice(&key).map_err(|_| HyperlineError::InvalidSignature)?;
    mac.update(headers.id.as_bytes());
    mac.update(b".");
    mac.update(headers.timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    // `webhook-signature` is a space-separated list of `v<version>,<base64>`.
    for part in headers.signature.split_whitespace() {
        let Some((_version, b64)) = part.split_once(',') else {
            continue;
        };
        let Ok(candidate) = B64.decode(b64) else {
            continue;
        };
        if candidate.len() == expected.len()
            && candidate.ct_eq(expected.as_slice()).unwrap_u8() == 1
        {
            return Ok(());
        }
    }
    Err(HyperlineError::InvalidSignature)
}

/// Minimal shape for dispatching a verified webhook payload.
#[derive(Debug, Deserialize)]
pub struct WebhookEnvelope {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical fixture: known key, payload, timestamp, id → known signature.
    fn sign(key: &[u8], id: &str, ts: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(key).unwrap();
        mac.update(id.as_bytes());
        mac.update(b".");
        mac.update(ts.as_bytes());
        mac.update(b".");
        mac.update(body);
        B64.encode(mac.finalize().into_bytes())
    }

    #[test]
    fn verifies_good_signature() {
        let key = b"super-secret-key-bytes";
        let secret = format!("{SECRET_PREFIX}{}", B64.encode(key));
        let id = "msg_01HY";
        let ts = "1712000000";
        let body = br#"{"type":"wallet.credited"}"#;
        let sig_b64 = sign(key, id, ts, body);
        let signature = format!("v1,{sig_b64}");

        verify_signature(
            &secret,
            WebhookHeaders {
                id,
                timestamp: ts,
                signature: &signature,
            },
            body,
            1712000010,
        )
        .unwrap();
    }

    #[test]
    fn rejects_tampered_body() {
        let key = b"super-secret-key-bytes";
        let secret = format!("{SECRET_PREFIX}{}", B64.encode(key));
        let id = "msg_01HY";
        let ts = "1712000000";
        let body = br#"{"type":"wallet.credited"}"#;
        let sig_b64 = sign(key, id, ts, body);
        let signature = format!("v1,{sig_b64}");

        let err = verify_signature(
            &secret,
            WebhookHeaders {
                id,
                timestamp: ts,
                signature: &signature,
            },
            br#"{"type":"wallet.debited"}"#,
            1712000010,
        )
        .unwrap_err();
        assert!(matches!(err, HyperlineError::InvalidSignature));
    }

    #[test]
    fn rejects_stale_timestamp() {
        let key = b"super-secret-key-bytes";
        let secret = format!("{SECRET_PREFIX}{}", B64.encode(key));
        let id = "msg_01HY";
        let ts = "1712000000";
        let body = br#"{}"#;
        let sig_b64 = sign(key, id, ts, body);
        let signature = format!("v1,{sig_b64}");

        let err = verify_signature(
            &secret,
            WebhookHeaders {
                id,
                timestamp: ts,
                signature: &signature,
            },
            body,
            1712000000 + TIMESTAMP_TOLERANCE_SECS + 1,
        )
        .unwrap_err();
        assert!(matches!(err, HyperlineError::StaleTimestamp));
    }

    #[test]
    fn accepts_second_signature_in_rotation() {
        // Hyperline may rotate secrets by sending multiple signatures.
        let key = b"current-key";
        let secret = format!("{SECRET_PREFIX}{}", B64.encode(key));
        let id = "msg_01HY";
        let ts = "1712000000";
        let body = br#"{}"#;
        let good = sign(key, id, ts, body);
        let signature = format!("v1,bogus-old v1,{good}");

        verify_signature(
            &secret,
            WebhookHeaders {
                id,
                timestamp: ts,
                signature: &signature,
            },
            body,
            1712000010,
        )
        .unwrap();
    }
}
