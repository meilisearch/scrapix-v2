//! PDF text + metadata extraction.
//!
//! Entry point for the opt-in PDF scraping feature (`features.pdf.enabled`).
//! Uses the pure-Rust `pdf-extract` crate — no native dependencies.
//!
//! This module is intentionally narrow: given raw PDF bytes it returns extracted
//! text, an optional title (from the PDF Info dictionary), and an optional
//! language hint (via whatlang on the extracted text). Scanned image PDFs
//! yield empty text and trigger a warn-level log so operators can spot the
//! quality gap without failing the crawl.
//!
//! Link extraction *from inside* PDFs is deliberately out of scope — see
//! the `extract_links` flag on `PdfConfig`, reserved for a follow-up issue.
//!
//! # Example
//!
//! ```rust,no_run
//! use scrapix_parser::pdf::{parse_pdf_bytes, PdfParseResult};
//!
//! let bytes: &[u8] = &[]; // raw PDF bytes from the fetcher
//! let result: PdfParseResult = parse_pdf_bytes(bytes, "https://example.com/doc.pdf")?;
//! assert!(!result.text.is_empty() || result.likely_scanned());
//! # Ok::<(), scrapix_core::ScrapixError>(())
//! ```

use scrapix_core::{Document, Result, ScrapixError};
use tracing::{debug, warn};
use url::Url;

use crate::language::detect_language;

/// Result of parsing a PDF's raw bytes.
#[derive(Debug, Clone)]
pub struct PdfParseResult {
    /// Extracted plain text, concatenated across pages.
    pub text: String,
    /// Title pulled from the PDF's Info dictionary, if present.
    pub title: Option<String>,
    /// Detected language (ISO 639-1), derived from the extracted text.
    pub language: Option<String>,
}

impl PdfParseResult {
    /// Heuristic: a PDF whose text extraction produced nothing is likely a
    /// scanned-image PDF (no OCR in v1). Callers can log a quality warning
    /// or skip the document entirely.
    pub fn likely_scanned(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Parse raw PDF bytes into text + metadata.
///
/// Returns `ScrapixError::Parse` for unparseable or encrypted PDFs, allowing
/// the content worker's existing failure path to publish a `PageFailed` event.
pub fn parse_pdf_bytes(bytes: &[u8], url: &str) -> Result<PdfParseResult> {
    if bytes.is_empty() {
        return Err(ScrapixError::Parse("Empty PDF body".to_string()));
    }

    // pdf-extract returns its own error type; map it into our domain error
    // so callers can treat PDF failures uniformly with HTML parse failures.
    let text = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| ScrapixError::Parse(format!("PDF text extraction failed: {}", e)))?;

    let text = normalize_pdf_text(&text);

    if text.trim().is_empty() {
        warn!(
            url = %url,
            bytes = bytes.len(),
            "PDF yielded no extractable text (likely scanned image); indexing with empty content"
        );
    } else {
        debug!(
            url = %url,
            bytes = bytes.len(),
            chars = text.len(),
            "Extracted text from PDF"
        );
    }

    let language = if text.trim().is_empty() {
        None
    } else {
        detect_language(&text)
    };

    Ok(PdfParseResult {
        text,
        title: None, // pdf-extract doesn't expose the Info dict; title is derived by the worker
        language,
    })
}

/// Build a ready-to-index `Document` from parsed PDF bytes.
///
/// Handles the PDF-specific indexing contract:
/// - `content` / `markdown` are set to the extracted text (PDFs have no
///   semantic heading hierarchy, so both fields hold the same plain text)
/// - `metadata.content_type = "application/pdf"` tags the doc so users can
///   `filter = "metadata.content_type = \"application/pdf\""` in Meilisearch
/// - `metadata.pdf_bytes` records the original byte size for debugging
/// - URL tags (path segments) follow the same convention as HTML/markdown pages
///
/// The `fallback_title` argument is used when the PDF itself carries no title;
/// the content worker typically passes the basename of the PDF URL.
pub fn build_pdf_document(
    url: &str,
    bytes_len: usize,
    parsed: PdfParseResult,
    fallback_title: Option<String>,
) -> Result<Document> {
    let parsed_url = Url::parse(url)?;
    let domain = parsed_url
        .host_str()
        .ok_or_else(|| ScrapixError::Parse("URL has no host".to_string()))?;

    let mut doc = Document::new(url, domain);

    // Title precedence: embedded PDF Info dict > fallback (URL basename) > None.
    doc.title = parsed.title.or(fallback_title);

    if !parsed.text.is_empty() {
        doc.content = Some(parsed.text.clone());
        doc.markdown = Some(parsed.text);
    }

    doc.language = parsed.language;

    // URL tags — mirrors parse_markdown_page so PDFs integrate with
    // hierarchical faceting the same way HTML pages do.
    let path = parsed_url.path();
    let mut tags = Vec::new();
    let mut current = String::new();
    for segment in path.split('/').filter(|s| !s.is_empty()) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(segment);
        tags.push(format!("/{}", current));
    }
    doc.urls_tags = Some(tags);

    // Stamp content_type so downstream consumers (Meilisearch filters,
    // analytics) can distinguish PDFs from HTML without reparsing.
    let mut metadata = std::collections::HashMap::new();
    metadata.insert("content_type".to_string(), "application/pdf".to_string());
    metadata.insert("pdf_bytes".to_string(), bytes_len.to_string());
    doc.metadata = Some(metadata);

    Ok(doc)
}

/// Derive a title from a PDF URL's basename.
/// Returns `None` if no meaningful name can be extracted.
pub fn title_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let last = parsed.path_segments()?.rfind(|s| !s.is_empty())?;
    let title = last.strip_suffix(".pdf").unwrap_or(last);
    let title = title.replace(['-', '_'], " ");
    let title = title.trim();
    if title.is_empty() {
        None
    } else {
        Some(title.to_string())
    }
}

/// Collapse excessive whitespace from pdf-extract output.
///
/// `pdf-extract` preserves layout whitespace (multiple spaces between
/// columns, stray newlines between wrapped lines). For indexing we want
/// compact text: keep paragraph breaks but normalize runs of whitespace.
fn normalize_pdf_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_newline = false;
    let mut prev_char_was_space = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Treat blank line as paragraph break (but collapse runs)
            if !last_was_newline && !out.is_empty() {
                out.push('\n');
                out.push('\n');
                last_was_newline = true;
            }
            prev_char_was_space = false;
            continue;
        }

        if !out.is_empty() && !last_was_newline {
            out.push(' ');
        }

        for c in trimmed.chars() {
            if c.is_whitespace() {
                if !prev_char_was_space {
                    out.push(' ');
                    prev_char_was_space = true;
                }
            } else {
                out.push(c);
                prev_char_was_space = false;
            }
        }

        last_was_newline = false;
    }

    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_title_from_url_basic() {
        assert_eq!(
            title_from_url("https://example.com/spec-v2.pdf"),
            Some("spec v2".to_string())
        );
    }

    #[test]
    fn test_title_from_url_no_extension() {
        assert_eq!(
            title_from_url("https://example.com/files/report"),
            Some("report".to_string())
        );
    }

    #[test]
    fn test_title_from_url_empty_path() {
        assert_eq!(title_from_url("https://example.com/"), None);
    }

    #[test]
    fn test_title_from_url_invalid() {
        assert_eq!(title_from_url("not a url"), None);
    }

    #[test]
    fn test_normalize_pdf_text_collapses_whitespace() {
        let raw = "Hello    world\n\n\n\nNext paragraph\nwith wrap";
        let normalized = normalize_pdf_text(raw);
        assert_eq!(normalized, "Hello world\n\nNext paragraph with wrap");
    }

    #[test]
    fn test_parse_pdf_bytes_empty_input() {
        let result = parse_pdf_bytes(&[], "https://example.com/doc.pdf");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pdf_bytes_invalid_input() {
        let result = parse_pdf_bytes(b"not a pdf", "https://example.com/doc.pdf");
        assert!(result.is_err());
    }

    #[test]
    fn test_likely_scanned_is_true_for_empty_text() {
        let r = PdfParseResult {
            text: String::new(),
            title: None,
            language: None,
        };
        assert!(r.likely_scanned());
    }

    #[test]
    fn test_build_pdf_document_sets_content_type_metadata() {
        let parsed = PdfParseResult {
            text: "Some extracted text".to_string(),
            title: Some("My PDF".to_string()),
            language: Some("en".to_string()),
        };
        let doc =
            build_pdf_document("https://example.com/docs/spec.pdf", 1024, parsed, None).unwrap();
        assert_eq!(doc.title.as_deref(), Some("My PDF"));
        assert_eq!(doc.content.as_deref(), Some("Some extracted text"));
        assert_eq!(doc.language.as_deref(), Some("en"));
        let meta = doc.metadata.expect("metadata must be set for PDFs");
        assert_eq!(
            meta.get("content_type").map(String::as_str),
            Some("application/pdf")
        );
        assert_eq!(meta.get("pdf_bytes").map(String::as_str), Some("1024"));
        let tags = doc.urls_tags.expect("urls_tags must be populated");
        assert!(tags.contains(&"/docs".to_string()));
    }

    #[test]
    fn test_build_pdf_document_uses_fallback_title() {
        let parsed = PdfParseResult {
            text: "hi".to_string(),
            title: None,
            language: None,
        };
        let doc = build_pdf_document(
            "https://example.com/a.pdf",
            10,
            parsed,
            Some("Fallback".to_string()),
        )
        .unwrap();
        assert_eq!(doc.title.as_deref(), Some("Fallback"));
    }
}
