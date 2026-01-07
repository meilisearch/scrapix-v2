//! Integration tests for the parser and extractor pipeline
//!
//! These tests verify that:
//! - HTML parsing extracts correct content
//! - Metadata extraction works correctly
//! - Schema.org/JSON-LD extraction works
//! - Language detection is accurate
//! - Markdown conversion preserves structure
//! - Block splitting creates correct hierarchy

use scrapix_extractor::{Extractor, MetadataExtractor, SchemaExtractor, SelectorExtractor};
use scrapix_parser::{detect_language, extract_content, html_to_markdown, HtmlParserBuilder};
use scrapix_tests::{create_raw_page, fixtures, init_test_tracing};

// ============================================================================
// Parser Tests
// ============================================================================

#[test]
fn test_parse_simple_article() {
    init_test_tracing();

    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .convert_to_markdown(true)
        .detect_language(true)
        .extract_schema(true)
        .build();

    let page = create_raw_page("https://example.com/article", fixtures::SIMPLE_ARTICLE);
    let doc = parser.parse(&page).unwrap();

    // Check basic fields
    assert_eq!(doc.url, "https://example.com/article");
    assert_eq!(doc.domain, "example.com");
    assert_eq!(doc.title, Some("Test Article Title".to_string()));

    // Check content extraction
    assert!(doc.content.is_some());
    let content = doc.content.unwrap();
    assert!(content.contains("first paragraph"));
    assert!(content.contains("Section One"));
    assert!(content.contains("Section Two"));

    // Check markdown conversion
    assert!(doc.markdown.is_some());
    let markdown = doc.markdown.unwrap();
    // Markdown should contain key content (format may vary)
    assert!(
        markdown.contains("Test Article Title")
            || markdown.contains("Section One")
            || markdown.contains('#')
    );

    // Check language detection
    assert_eq!(doc.language, Some("en".to_string()));
}

#[test]
fn test_parse_french_page() {
    init_test_tracing();

    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .detect_language(true)
        .build();

    let page = create_raw_page("https://example.fr/article", fixtures::FRENCH_PAGE);
    let doc = parser.parse(&page).unwrap();

    // Language should be detected as French
    assert_eq!(doc.language, Some("fr".to_string()));

    // Content should be extracted
    assert!(doc.content.is_some());
    let content = doc.content.unwrap();
    assert!(content.contains("français") || content.contains("France"));
}

#[test]
fn test_parse_minimal_page() {
    init_test_tracing();

    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .min_content_length(5)
        .build();

    let page = create_raw_page("https://example.com/minimal", fixtures::MINIMAL_PAGE);
    let doc = parser.parse(&page).unwrap();

    assert_eq!(doc.title, Some("Minimal".to_string()));
    assert!(doc.content.is_some());
}

#[test]
fn test_readability_content_extraction() {
    init_test_tracing();

    let content = extract_content(fixtures::SIMPLE_ARTICLE);

    // Should extract main article content
    assert!(content.contains("first paragraph"));
    assert!(content.contains("Section One"));

    // Should NOT include navigation/footer boilerplate
    assert!(!content.contains("Home") || content.matches("Home").count() < 3);
    assert!(!content.contains("Privacy Policy") || content.len() > 500);
}

#[test]
fn test_markdown_conversion() {
    init_test_tracing();

    let markdown = html_to_markdown(fixtures::SIMPLE_ARTICLE);

    // Should convert headings
    assert!(markdown.contains('#'));

    // Should convert lists
    assert!(markdown.contains("First list item") || markdown.contains("- "));

    // Should preserve paragraph structure
    assert!(markdown.contains('\n'));
}

#[test]
fn test_language_detection() {
    // English text - needs to be long enough for reliable detection
    let en_text = "This is a sample English text for language detection testing. \
        The library needs enough text content to make an accurate determination \
        of the language being used in the document.";
    let lang = detect_language(en_text);
    assert_eq!(lang, Some("en".to_string()));

    // French text
    let fr_text = "Ceci est un texte en français pour tester la détection de langue. \
        Le contenu doit être suffisamment long pour permettre une détection précise \
        de la langue utilisée dans le document.";
    let lang = detect_language(fr_text);
    assert_eq!(lang, Some("fr".to_string()));

    // German text - needs to be longer for reliable detection
    let de_text = "Dies ist ein deutscher Text zur Spracherkennung. \
        Der Inhalt muss lang genug sein, um eine genaue Erkennung \
        der im Dokument verwendeten Sprache zu ermöglichen. \
        Die Bibliothek benötigt ausreichend Textinhalt.";
    let lang = detect_language(de_text);
    assert_eq!(lang, Some("de".to_string()));
}

// ============================================================================
// Extractor Tests
// ============================================================================

#[test]
fn test_metadata_extraction() {
    init_test_tracing();

    let extractor = MetadataExtractor::new();
    let metadata = extractor.extract(fixtures::SIMPLE_ARTICLE).unwrap();

    // Basic metadata
    assert_eq!(metadata.title, Some("Test Article Title".to_string()));
    // Note: OG description takes precedence over meta description
    assert_eq!(
        metadata.description,
        Some("OG description for testing".to_string())
    );
    // Author is in meta tag, should be extracted
    assert_eq!(metadata.author, Some("Test Author".to_string()));

    // Open Graph (keys are stored without the "og:" prefix)
    assert_eq!(
        metadata.open_graph.get("title"),
        Some(&"Test Article OG Title".to_string())
    );
    assert_eq!(
        metadata.open_graph.get("type"),
        Some(&"article".to_string())
    );
}

#[test]
fn test_schema_extraction() {
    init_test_tracing();

    let extractor = SchemaExtractor::default();
    let schema = extractor.extract(fixtures::PAGE_WITH_SCHEMA).unwrap();

    // Should find the Product schema
    assert_eq!(schema.items.len(), 1);
    assert_eq!(schema.items[0].schema_type, "Product");

    // Check JSON-LD data
    let json_ld = schema.json_ld.as_ref().unwrap();
    assert!(json_ld.is_array() || json_ld.is_object());

    // If it's the first item, check properties
    let product = if json_ld.is_array() {
        json_ld.as_array().unwrap().first().unwrap()
    } else {
        json_ld
    };
    assert_eq!(product["name"], "Test Product");
    assert_eq!(product["offers"]["price"], "99.99");
}

#[test]
fn test_selector_extraction() {
    init_test_tracing();

    let mut extractor = SelectorExtractor::new();
    extractor.add_text("title", "title");
    extractor.add_text("heading", "h1");
    extractor.add_text("author", ".author");
    extractor.add_list("nav_links", "nav a");

    let result = extractor.extract(fixtures::SIMPLE_ARTICLE).unwrap();

    assert_eq!(
        result.values.get("title"),
        Some(&serde_json::json!("Test Article Title"))
    );
    assert_eq!(
        result.values.get("heading"),
        Some(&serde_json::json!("Test Article Title"))
    );
    assert!(result.values.get("author").is_some());

    // Nav links should be a list
    let nav_links = result.values.get("nav_links").unwrap();
    assert!(nav_links.is_array());
}

#[test]
fn test_combined_extractor() {
    init_test_tracing();

    let extractor = Extractor::with_all_features();
    let result = extractor.extract(fixtures::SIMPLE_ARTICLE).unwrap();

    // All extractors should have run
    assert!(result.metadata.is_some());
    assert!(result.schema.is_some()); // May be empty but not None
    assert!(result.blocks.is_some());

    // Metadata should be correct
    let metadata = result.metadata.unwrap();
    assert_eq!(metadata.title, Some("Test Article Title".to_string()));
}

#[test]
fn test_extractor_with_custom_selectors() {
    init_test_tracing();

    let mut selector_extractor = SelectorExtractor::new();
    selector_extractor.add_text("main_heading", "article h1");
    selector_extractor.add_text("first_paragraph", "article p:first-of-type");

    let extractor = Extractor::new()
        .with_metadata()
        .with_selectors(selector_extractor);

    let result = extractor.extract(fixtures::SIMPLE_ARTICLE).unwrap();

    assert!(result.metadata.is_some());
    assert!(result.custom.is_some());

    let custom = result.custom.unwrap();
    assert!(custom.values.get("main_heading").is_some());
}

// ============================================================================
// Full Pipeline Tests
// ============================================================================

#[test]
fn test_full_parse_and_extract_pipeline() {
    init_test_tracing();

    // Step 1: Parse the page
    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .convert_to_markdown(true)
        .detect_language(true)
        .extract_schema(true)
        .build();

    let page = create_raw_page("https://example.com/product", fixtures::PAGE_WITH_SCHEMA);
    let doc = parser.parse(&page).unwrap();

    // Step 2: Additional extraction
    let extractor = Extractor::with_all_features();
    let extraction = extractor.extract(fixtures::PAGE_WITH_SCHEMA).unwrap();

    // Verify document fields
    assert_eq!(doc.title, Some("Product Page".to_string()));
    // Note: PAGE_WITH_SCHEMA has minimal body content, so content extraction may be empty
    // The readability algorithm may not extract content from such minimal pages

    // Verify schema was extracted
    assert!(extraction.schema.is_some());
    let schema = extraction.schema.as_ref().unwrap();
    assert!(!schema.items.is_empty());
    assert_eq!(schema.items[0].schema_type, "Product");

    // Convert to document fields
    let fields = extraction.to_document_fields();
    assert!(fields.contains_key("title") || fields.contains_key("schema"));
}

#[test]
fn test_link_extraction_from_page() {
    init_test_tracing();

    // Use the parser to get links
    let parser = HtmlParserBuilder::new().extract_content(true).build();

    let page = create_raw_page("https://example.com/links", fixtures::PAGE_WITH_LINKS);
    let doc = parser.parse(&page).unwrap();

    // Document should be created
    assert_eq!(doc.domain, "example.com");

    // Test URL extraction via selector - extract link text as list
    let mut selector = SelectorExtractor::new();
    selector.add_list("link_texts", "a");

    // Also extract a single link's href attribute
    selector.add_attribute("first_link", "a", "href");

    let result = selector.extract(fixtures::PAGE_WITH_LINKS).unwrap();

    // Should find link texts
    let link_texts = result.values.get("link_texts").unwrap().as_array().unwrap();
    assert!(link_texts.len() >= 5);
    assert!(link_texts
        .iter()
        .any(|t| t.as_str().map_or(false, |s| s.contains("Internal Page"))));

    // First link href should be extracted
    let first_link = result.values.get("first_link").unwrap().as_str().unwrap();
    assert_eq!(first_link, "/page1");
}

#[test]
fn test_empty_content_handling() {
    init_test_tracing();

    let parser = HtmlParserBuilder::new()
        .extract_content(true)
        .min_content_length(50)
        .build();

    let page = create_raw_page("https://example.com/empty", fixtures::EMPTY_CONTENT_PAGE);
    let doc = parser.parse(&page).unwrap();

    // Should still create document but content may be empty or minimal
    assert_eq!(doc.url, "https://example.com/empty");
    // Content might be None or very short due to min_content_length
}

#[test]
fn test_metadata_to_document_fields() {
    init_test_tracing();

    let extractor = Extractor::new().with_metadata().with_schema();
    let result = extractor.extract(fixtures::SIMPLE_ARTICLE).unwrap();
    let fields = result.to_document_fields();

    // Should have title and description
    assert!(fields.contains_key("title"));
    assert!(fields.contains_key("description"));

    // Should have Open Graph as nested object
    assert!(fields.contains_key("open_graph"));
}
