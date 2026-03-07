//! Block splitting by headings for HTML documents
//!
//! Splits HTML content into semantic blocks based on heading hierarchy (H1-H6).
//! Useful for creating multiple searchable documents from a single page.

use scraper::{ElementRef, Html, Node, Selector};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, instrument};

/// Errors that can occur during block extraction
#[derive(Debug, Error)]
pub enum BlockError {
    #[error("Invalid selector: {0}")]
    InvalidSelector(String),

    #[error("Extraction error: {0}")]
    ExtractionError(String),
}

/// Configuration for block splitting
#[derive(Debug, Clone)]
pub struct BlockConfig {
    /// Minimum heading level to split on (1-6, default: 2)
    pub min_level: u8,

    /// Maximum heading level to split on (1-6, default: 6)
    pub max_level: u8,

    /// CSS selector for the content container (default: article, main, .content, body)
    pub content_selector: Option<String>,

    /// Minimum content length for a block (default: 50)
    pub min_content_length: usize,

    /// Include parent heading hierarchy in blocks
    pub include_hierarchy: bool,

    /// Extract anchor/id from headings
    pub extract_anchors: bool,

    /// Maximum blocks to extract (0 = unlimited)
    pub max_blocks: usize,
}

impl Default for BlockConfig {
    fn default() -> Self {
        Self {
            min_level: 2,
            max_level: 6,
            content_selector: None,
            min_content_length: 50,
            include_hierarchy: true,
            extract_anchors: true,
            max_blocks: 0,
        }
    }
}

/// A single content block extracted from the document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    /// Block index (0-based)
    pub index: u32,

    /// Block content (text)
    pub content: String,

    /// Block content as Markdown (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub markdown: Option<String>,

    /// Heading hierarchy (h1 -> h6)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h2: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h3: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h4: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h5: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub h6: Option<String>,

    /// Anchor/fragment ID for this block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,

    /// Heading text that starts this block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,

    /// Heading level (1-6)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_level: Option<u8>,
}

impl ContentBlock {
    /// Create a new empty block
    pub fn new(index: u32) -> Self {
        Self {
            index,
            content: String::new(),
            markdown: None,
            h1: None,
            h2: None,
            h3: None,
            h4: None,
            h5: None,
            h6: None,
            anchor: None,
            heading: None,
            heading_level: None,
        }
    }

    /// Set the heading for a specific level
    pub fn set_heading(&mut self, level: u8, text: String) {
        match level {
            1 => self.h1 = Some(text),
            2 => self.h2 = Some(text),
            3 => self.h3 = Some(text),
            4 => self.h4 = Some(text),
            5 => self.h5 = Some(text),
            6 => self.h6 = Some(text),
            _ => {}
        }
    }

    /// Copy heading hierarchy from another block
    pub fn copy_hierarchy_from(&mut self, other: &ContentBlock, up_to_level: u8) {
        if up_to_level >= 1 {
            self.h1 = other.h1.clone();
        }
        if up_to_level >= 2 {
            self.h2 = other.h2.clone();
        }
        if up_to_level >= 3 {
            self.h3 = other.h3.clone();
        }
        if up_to_level >= 4 {
            self.h4 = other.h4.clone();
        }
        if up_to_level >= 5 {
            self.h5 = other.h5.clone();
        }
        if up_to_level >= 6 {
            self.h6 = other.h6.clone();
        }
    }

    /// Clear headings below a certain level
    pub fn clear_below_level(&mut self, level: u8) {
        if level < 2 {
            self.h2 = None;
        }
        if level < 3 {
            self.h3 = None;
        }
        if level < 4 {
            self.h4 = None;
        }
        if level < 5 {
            self.h5 = None;
        }
        if level < 6 {
            self.h6 = None;
        }
    }

    /// Check if block has meaningful content
    pub fn has_content(&self, min_length: usize) -> bool {
        self.content.trim().len() >= min_length
    }
}

/// Result of block extraction
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedBlocks {
    /// Extracted content blocks
    pub blocks: Vec<ContentBlock>,

    /// Total number of blocks
    pub count: usize,

    /// Whether blocks were truncated due to max_blocks
    pub truncated: bool,
}

/// Block splitter for HTML documents
pub struct BlockSplitter {
    config: BlockConfig,
    content_selectors: Vec<Selector>,
}

impl Default for BlockSplitter {
    fn default() -> Self {
        Self::new(BlockConfig::default())
    }
}

impl BlockSplitter {
    /// Create a new block splitter with configuration
    pub fn new(config: BlockConfig) -> Self {
        // Build content selectors
        let content_selector_strs = if let Some(ref selector) = config.content_selector {
            vec![selector.clone()]
        } else {
            vec![
                "article".to_string(),
                "main".to_string(),
                "[role=\"main\"]".to_string(),
                ".content".to_string(),
                ".article-content".to_string(),
                ".post-content".to_string(),
                ".entry-content".to_string(),
                "#content".to_string(),
                "body".to_string(),
            ]
        };

        let content_selectors: Vec<Selector> = content_selector_strs
            .iter()
            .filter_map(|s| Selector::parse(s).ok())
            .collect();

        Self {
            config,
            content_selectors,
        }
    }

    /// Create splitter with default config
    pub fn with_defaults() -> Self {
        Self::default()
    }

    /// Create splitter that splits on H2 headings only
    pub fn split_on_h2() -> Self {
        Self::new(BlockConfig {
            min_level: 2,
            max_level: 2,
            ..Default::default()
        })
    }

    /// Create splitter that splits on all headings (H1-H6)
    pub fn split_on_all() -> Self {
        Self::new(BlockConfig {
            min_level: 1,
            max_level: 6,
            ..Default::default()
        })
    }

    /// Split HTML into content blocks
    #[instrument(skip(self, html), level = "debug")]
    pub fn split(&self, html: &str) -> Result<ExtractedBlocks, BlockError> {
        let document = Html::parse_document(html);
        self.split_from_dom(&document)
    }

    /// Split a pre-parsed DOM into content blocks, avoiding redundant parsing
    pub fn split_from_dom(&self, document: &Html) -> Result<ExtractedBlocks, BlockError> {
        let mut result = ExtractedBlocks::default();

        // Find content container
        let content_root = self.find_content_root(document);

        // Extract blocks
        let blocks = match content_root {
            Some(root) => self.extract_blocks_from_element(&root),
            None => self.extract_blocks_from_document(document),
        };

        // Filter and limit blocks
        let filtered: Vec<ContentBlock> = blocks
            .into_iter()
            .filter(|b| b.has_content(self.config.min_content_length))
            .enumerate()
            .map(|(i, mut b)| {
                b.index = i as u32;
                b
            })
            .take(if self.config.max_blocks > 0 {
                self.config.max_blocks
            } else {
                usize::MAX
            })
            .collect();

        result.truncated = self.config.max_blocks > 0 && filtered.len() >= self.config.max_blocks;
        result.count = filtered.len();
        result.blocks = filtered;

        debug!(
            block_count = result.count,
            truncated = result.truncated,
            "Split document into blocks"
        );

        Ok(result)
    }

    /// Find the main content container
    fn find_content_root<'a>(&self, document: &'a Html) -> Option<ElementRef<'a>> {
        for selector in &self.content_selectors {
            if let Some(element) = document.select(selector).next() {
                // Skip body if we have a more specific match
                if element.value().name() != "body" {
                    return Some(element);
                }
            }
        }
        // Fall back to body
        let body_selector = self.content_selectors.last()?;
        document.select(body_selector).next()
    }

    /// Extract blocks from a specific element
    fn extract_blocks_from_element(&self, element: &ElementRef) -> Vec<ContentBlock> {
        let mut blocks = Vec::new();
        let mut current_block = ContentBlock::new(0);
        let mut heading_hierarchy = HeadingHierarchy::new();

        self.traverse_element(
            element,
            &mut blocks,
            &mut current_block,
            &mut heading_hierarchy,
        );

        // Don't forget the last block
        if current_block.has_content(0) {
            blocks.push(current_block);
        }

        blocks
    }

    /// Extract blocks from the full document
    fn extract_blocks_from_document(&self, document: &Html) -> Vec<ContentBlock> {
        use std::sync::OnceLock;
        static BODY_SELECTOR: OnceLock<Selector> = OnceLock::new();
        let body_selector =
            BODY_SELECTOR.get_or_init(|| Selector::parse("body").expect("valid body selector"));
        if let Some(body) = document.select(&body_selector).next() {
            self.extract_blocks_from_element(&body)
        } else {
            Vec::new()
        }
    }

    /// Traverse an element and extract blocks
    fn traverse_element(
        &self,
        element: &ElementRef,
        blocks: &mut Vec<ContentBlock>,
        current_block: &mut ContentBlock,
        hierarchy: &mut HeadingHierarchy,
    ) {
        for child in element.children() {
            match child.value() {
                Node::Element(el) => {
                    let tag_name = el.name();

                    // Check if this is any heading (H1-H6)
                    if let Some(level) = self.get_heading_level(tag_name) {
                        let child_ref = ElementRef::wrap(child).unwrap();
                        let heading_text = child_ref.text().collect::<String>().trim().to_string();

                        // Always track headings in hierarchy for context
                        if self.config.include_hierarchy {
                            hierarchy.set(level, heading_text.clone());

                            // Update current block's hierarchy if it doesn't have this level yet
                            if current_block.h1.is_none() && level == 1 {
                                current_block.h1 = Some(heading_text.clone());
                            }
                        }

                        // Only split on headings within configured range
                        if level >= self.config.min_level && level <= self.config.max_level {
                            // Save current block if it has content
                            if current_block.has_content(0) {
                                blocks.push(current_block.clone());
                            }

                            let anchor = if self.config.extract_anchors {
                                el.attr("id").map(|s| s.to_string())
                            } else {
                                None
                            };

                            // Start new block
                            *current_block = ContentBlock::new(blocks.len() as u32);
                            current_block.heading = Some(heading_text.clone());
                            current_block.heading_level = Some(level);
                            current_block.anchor = anchor;

                            // Copy hierarchy
                            if self.config.include_hierarchy {
                                current_block.h1 = hierarchy.h1.clone();
                                current_block.h2 = hierarchy.h2.clone();
                                current_block.h3 = hierarchy.h3.clone();
                                current_block.h4 = hierarchy.h4.clone();
                                current_block.h5 = hierarchy.h5.clone();
                                current_block.h6 = hierarchy.h6.clone();
                            }

                            continue;
                        }
                    }

                    // Skip script, style, nav, footer, etc.
                    if self.should_skip_element(tag_name) {
                        continue;
                    }

                    // Recursively process children
                    if let Some(child_ref) = ElementRef::wrap(child) {
                        self.traverse_element(&child_ref, blocks, current_block, hierarchy);
                    }
                }
                Node::Text(text) => {
                    let text_content = text.trim();
                    if !text_content.is_empty() {
                        if !current_block.content.is_empty() {
                            current_block.content.push(' ');
                        }
                        current_block.content.push_str(text_content);
                    }
                }
                _ => {}
            }
        }
    }

    /// Get heading level from tag name (returns None if not a heading)
    fn get_heading_level(&self, tag_name: &str) -> Option<u8> {
        match tag_name {
            "h1" => Some(1),
            "h2" => Some(2),
            "h3" => Some(3),
            "h4" => Some(4),
            "h5" => Some(5),
            "h6" => Some(6),
            _ => None,
        }
    }

    /// Check if element should be skipped
    fn should_skip_element(&self, tag_name: &str) -> bool {
        matches!(
            tag_name,
            "script"
                | "style"
                | "nav"
                | "footer"
                | "header"
                | "aside"
                | "noscript"
                | "iframe"
                | "svg"
                | "canvas"
                | "form"
                | "button"
                | "input"
                | "select"
                | "textarea"
        )
    }
}

/// Helper to track heading hierarchy
struct HeadingHierarchy {
    h1: Option<String>,
    h2: Option<String>,
    h3: Option<String>,
    h4: Option<String>,
    h5: Option<String>,
    h6: Option<String>,
}

impl HeadingHierarchy {
    fn new() -> Self {
        Self {
            h1: None,
            h2: None,
            h3: None,
            h4: None,
            h5: None,
            h6: None,
        }
    }

    fn set(&mut self, level: u8, text: String) {
        match level {
            1 => {
                self.h1 = Some(text);
                self.h2 = None;
                self.h3 = None;
                self.h4 = None;
                self.h5 = None;
                self.h6 = None;
            }
            2 => {
                self.h2 = Some(text);
                self.h3 = None;
                self.h4 = None;
                self.h5 = None;
                self.h6 = None;
            }
            3 => {
                self.h3 = Some(text);
                self.h4 = None;
                self.h5 = None;
                self.h6 = None;
            }
            4 => {
                self.h4 = Some(text);
                self.h5 = None;
                self.h6 = None;
            }
            5 => {
                self.h5 = Some(text);
                self.h6 = None;
            }
            6 => {
                self.h6 = Some(text);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_basic() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <article>
                    <h1>Main Title</h1>
                    <p>Introduction paragraph with enough content to pass the minimum length check.</p>

                    <h2>Section 1</h2>
                    <p>Content for section 1 with sufficient text to be considered a valid block.</p>

                    <h2>Section 2</h2>
                    <p>Content for section 2 also with enough meaningful content here.</p>
                </article>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::with_defaults();
        let result = splitter.split(html).unwrap();

        assert_eq!(result.count, 3);
        assert_eq!(result.blocks[0].h1, Some("Main Title".to_string()));
        assert_eq!(result.blocks[1].heading, Some("Section 1".to_string()));
        assert_eq!(result.blocks[2].heading, Some("Section 2".to_string()));
    }

    #[test]
    fn test_split_with_hierarchy() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h1>Document Title</h1>
                <p>Some intro content that is long enough to pass validation checks.</p>

                <h2>Chapter 1</h2>
                <p>Chapter content with sufficient length to be included in results.</p>

                <h3>Section 1.1</h3>
                <p>Section content that also meets the minimum length requirement.</p>

                <h2>Chapter 2</h2>
                <p>Another chapter with its own content that passes length checks.</p>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::new(BlockConfig {
            min_level: 1,
            max_level: 6,
            min_content_length: 20,
            ..Default::default()
        });
        let result = splitter.split(html).unwrap();

        // Find Section 1.1 block
        let section_block = result
            .blocks
            .iter()
            .find(|b| b.heading == Some("Section 1.1".to_string()));

        assert!(section_block.is_some());
        let section = section_block.unwrap();

        // Should have parent hierarchy
        assert_eq!(section.h1, Some("Document Title".to_string()));
        assert_eq!(section.h2, Some("Chapter 1".to_string()));
        assert_eq!(section.h3, Some("Section 1.1".to_string()));
    }

    #[test]
    fn test_split_with_anchors() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h2 id="getting-started">Getting Started</h2>
                <p>This section explains how to get started with the framework properly.</p>

                <h2 id="installation">Installation</h2>
                <p>Here we cover the installation process in detail for all platforms.</p>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::with_defaults();
        let result = splitter.split(html).unwrap();

        assert_eq!(result.count, 2);
        assert_eq!(result.blocks[0].anchor, Some("getting-started".to_string()));
        assert_eq!(result.blocks[1].anchor, Some("installation".to_string()));
    }

    #[test]
    fn test_split_min_content_filter() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h2>Section with Content</h2>
                <p>This section has substantial content that exceeds the minimum length requirement for blocks.</p>

                <h2>Empty Section</h2>
                <p>Short</p>

                <h2>Another Section with Content</h2>
                <p>This section also has meaningful content that should be included in the results.</p>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::new(BlockConfig {
            min_content_length: 30,
            ..Default::default()
        });
        let result = splitter.split(html).unwrap();

        // Empty section should be filtered out
        assert_eq!(result.count, 2);
    }

    #[test]
    fn test_split_max_blocks() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h2>Section 1</h2>
                <p>Content for section one with enough text to pass validation.</p>

                <h2>Section 2</h2>
                <p>Content for section two with enough text to pass validation.</p>

                <h2>Section 3</h2>
                <p>Content for section three with enough text to pass validation.</p>

                <h2>Section 4</h2>
                <p>Content for section four with enough text to pass validation.</p>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::new(BlockConfig {
            max_blocks: 2,
            min_content_length: 10,
            ..Default::default()
        });
        let result = splitter.split(html).unwrap();

        assert_eq!(result.count, 2);
        assert!(result.truncated);
    }

    #[test]
    fn test_split_on_h2_only() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h1>Title</h1>
                <p>Some content that should be captured for the first block here.</p>

                <h2>Section</h2>
                <p>Section content that should be in its own separate block.</p>

                <h3>Subsection</h3>
                <p>Subsection content that should be included with the section above.</p>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::split_on_h2();
        let result = splitter.split(html).unwrap();

        // Should only split on H2, so H3 content should be part of H2 block
        assert_eq!(result.count, 2);

        // Second block should contain both H2 and H3 content
        let section_block = &result.blocks[1];
        assert!(section_block.content.contains("Subsection content"));
    }

    #[test]
    fn test_skips_nav_footer() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <nav>
                    <h2>Navigation</h2>
                    <p>Nav content that should be skipped entirely from extraction.</p>
                </nav>

                <main>
                    <h2>Main Content</h2>
                    <p>This is the main content that should be extracted properly.</p>
                </main>

                <footer>
                    <h2>Footer</h2>
                    <p>Footer content that should also be skipped from extraction.</p>
                </footer>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::with_defaults();
        let result = splitter.split(html).unwrap();

        // Should only have main content
        assert_eq!(result.count, 1);
        assert_eq!(result.blocks[0].heading, Some("Main Content".to_string()));
    }

    #[test]
    fn test_empty_document() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
            </body>
            </html>
        "#;

        let splitter = BlockSplitter::with_defaults();
        let result = splitter.split(html).unwrap();

        assert_eq!(result.count, 0);
        assert!(!result.truncated);
    }
}
