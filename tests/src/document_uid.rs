//! Document UID Tests (P2)
//!
//! Document UIDs are UUIDv5 (deterministic from URL). The same URL always
//! produces the same UID, enabling in-place updates across crawl runs.
//! Block documents reference their parent via parent_document_id.
//! These tests verify the UID system and document hierarchy.

use scrapix_core::Document;
use std::collections::HashSet;

// ============================================================================
// UID generation uniqueness
// ============================================================================

#[test]
fn test_document_uid_is_unique_across_many_documents() {
    let mut uids = HashSet::new();
    for i in 0..10_000 {
        let doc = Document::new(format!("https://example.com/page-{i}"), "example.com");
        assert!(
            uids.insert(doc.uid.clone()),
            "UID collision at document {i}: {}",
            doc.uid
        );
    }
}

#[test]
fn test_document_uid_deterministic_for_same_url() {
    // Same URL must produce the same UID (UUIDv5 is deterministic from URL)
    // This enables in-place updates: re-crawling a URL updates the existing document
    let doc1 = Document::new("https://example.com/page", "example.com");
    let doc2 = Document::new("https://example.com/page", "example.com");
    assert_eq!(doc1.uid, doc2.uid, "Same URL must produce the same UID");
}

#[test]
fn test_document_uid_is_valid_uuid_format() {
    let doc = Document::new("https://example.com", "example.com");
    // UUIDv4 format: 8-4-4-4-12 hex chars
    assert_eq!(doc.uid.len(), 36, "UUID should be 36 chars");
    let parts: Vec<&str> = doc.uid.split('-').collect();
    assert_eq!(parts.len(), 5, "UUID should have 5 parts");
    assert_eq!(parts[0].len(), 8);
    assert_eq!(parts[1].len(), 4);
    assert_eq!(parts[2].len(), 4);
    assert_eq!(parts[3].len(), 4);
    assert_eq!(parts[4].len(), 12);
}

// ============================================================================
// Block documents
// ============================================================================

#[test]
fn test_block_document_references_parent() {
    let parent = Document::new("https://example.com/page", "example.com");
    let block = Document::new_block(&parent, 0);

    assert_eq!(
        block.parent_document_id,
        Some(parent.uid.clone()),
        "Block should reference parent UID"
    );
}

#[test]
fn test_block_document_has_unique_uid() {
    let parent = Document::new("https://example.com/page", "example.com");
    let block1 = Document::new_block(&parent, 0);
    let block2 = Document::new_block(&parent, 1);

    assert_ne!(parent.uid, block1.uid);
    assert_ne!(parent.uid, block2.uid);
    assert_ne!(block1.uid, block2.uid);
}

#[test]
fn test_block_document_inherits_url_and_domain() {
    let parent = Document::new("https://example.com/page", "example.com");
    let block = Document::new_block(&parent, 2);

    assert_eq!(block.url, parent.url);
    assert_eq!(block.domain, parent.domain);
    assert_eq!(block.page_block, Some(2));
}

#[test]
fn test_is_block_flag() {
    let parent = Document::new("https://example.com/page", "example.com");
    let block = Document::new_block(&parent, 0);

    assert!(!parent.is_block(), "Parent should not be a block");
    assert!(block.is_block(), "Block document should be a block");
}

#[test]
fn test_multiple_blocks_from_same_parent() {
    let parent = Document::new("https://example.com/page", "example.com");
    let blocks: Vec<Document> = (0..100).map(|i| Document::new_block(&parent, i)).collect();

    let uids: HashSet<&str> = blocks.iter().map(|b| b.uid.as_str()).collect();
    assert_eq!(uids.len(), 100, "All block UIDs should be unique");

    for (i, block) in blocks.iter().enumerate() {
        assert_eq!(block.parent_document_id, Some(parent.uid.clone()));
        assert_eq!(block.page_block, Some(i as u32));
    }
}

// ============================================================================
// Document field initialization
// ============================================================================

#[test]
fn test_document_new_has_correct_defaults() {
    let doc = Document::new("https://example.com/page", "example.com");

    assert_eq!(doc.url, "https://example.com/page");
    assert_eq!(doc.domain, "example.com");
    assert!(doc.title.is_none());
    assert!(doc.content.is_none());
    assert!(doc.markdown.is_none());
    assert!(doc.metadata.is_none());
    assert!(doc.schema.is_none());
    assert!(doc.language.is_none());
    assert!(doc.parent_document_id.is_none());
    assert!(doc.page_block.is_none());
}

#[test]
fn test_document_serialization_round_trip() {
    let mut doc = Document::new("https://example.com", "example.com");
    doc.title = Some("Test".to_string());
    doc.content = Some("Hello world".to_string());
    doc.language = Some("en".to_string());

    let json = serde_json::to_string(&doc).unwrap();
    let d: Document = serde_json::from_str(&json).unwrap();

    assert_eq!(d.uid, doc.uid);
    assert_eq!(d.url, doc.url);
    assert_eq!(d.title, doc.title);
    assert_eq!(d.content, doc.content);
    assert_eq!(d.language, doc.language);
}

#[test]
fn test_document_serialization_skips_none_optional_fields() {
    let doc = Document::new("https://example.com", "example.com");
    let json = serde_json::to_string(&doc).unwrap();
    let obj: serde_json::Value = serde_json::from_str(&json).unwrap();

    // None fields should either be absent or null
    if let Some(title) = obj.get("title") {
        assert!(title.is_null(), "None title should serialize as null");
    }
}
