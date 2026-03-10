//! Readability-style content extraction
//!
//! Extracts the main content from a web page by removing boilerplate
//! (navigation, ads, footers, etc.) and keeping the main article content.

use std::sync::OnceLock;

use scraper::{ElementRef, Html, Selector};

// Pre-compiled selectors used in readability extraction (compiled once on first use)
static LI_SELECTOR: OnceLock<Selector> = OnceLock::new();
static P_SELECTOR: OnceLock<Selector> = OnceLock::new();
static A_SELECTOR: OnceLock<Selector> = OnceLock::new();
static DIV_SECTION_ARTICLE_SELECTOR: OnceLock<Selector> = OnceLock::new();
static BODY_SELECTOR: OnceLock<Selector> = OnceLock::new();

fn li_selector() -> &'static Selector {
    LI_SELECTOR.get_or_init(|| Selector::parse("li").unwrap())
}

fn p_selector() -> &'static Selector {
    P_SELECTOR.get_or_init(|| Selector::parse("p").unwrap())
}

fn a_selector() -> &'static Selector {
    A_SELECTOR.get_or_init(|| Selector::parse("a").unwrap())
}

fn div_section_article_selector() -> &'static Selector {
    DIV_SECTION_ARTICLE_SELECTOR.get_or_init(|| Selector::parse("div, section, article").unwrap())
}

fn body_selector() -> &'static Selector {
    BODY_SELECTOR.get_or_init(|| Selector::parse("body").unwrap())
}

/// Configuration for content extraction
#[derive(Debug, Clone)]
pub struct ReadabilityConfig {
    /// Minimum paragraph length to consider
    pub min_paragraph_length: usize,
    /// Minimum text density (text/total characters ratio)
    pub min_text_density: f64,
    /// Tags to remove completely
    pub remove_tags: Vec<String>,
    /// Class names that indicate non-content
    pub negative_classes: Vec<String>,
    /// Class names that indicate content
    pub positive_classes: Vec<String>,
}

impl Default for ReadabilityConfig {
    fn default() -> Self {
        Self {
            min_paragraph_length: 25,
            min_text_density: 0.3,
            remove_tags: vec![
                "head".to_string(),
                "script".to_string(),
                "style".to_string(),
                "noscript".to_string(),
                "iframe".to_string(),
                "svg".to_string(),
                "nav".to_string(),
                "footer".to_string(),
                "header".to_string(),
                "aside".to_string(),
                "form".to_string(),
                "button".to_string(),
                "input".to_string(),
                "select".to_string(),
                "textarea".to_string(),
            ],
            negative_classes: vec![
                "sidebar".to_string(),
                "nav".to_string(),
                "navigation".to_string(),
                "menu".to_string(),
                "footer".to_string(),
                "header".to_string(),
                "comment".to_string(),
                "comments".to_string(),
                "ad".to_string(),
                "ads".to_string(),
                "advertisement".to_string(),
                "social".to_string(),
                "share".to_string(),
                "related".to_string(),
                "recommended".to_string(),
                "popular".to_string(),
                "trending".to_string(),
                "breadcrumb".to_string(),
                "pagination".to_string(),
                "widget".to_string(),
            ],
            positive_classes: vec![
                "article".to_string(),
                "content".to_string(),
                "main".to_string(),
                "post".to_string(),
                "entry".to_string(),
                "text".to_string(),
                "body".to_string(),
                "story".to_string(),
            ],
        }
    }
}

/// Extract main content from HTML
pub fn extract_content(html: &str) -> String {
    extract_content_with_config(html, &ReadabilityConfig::default())
}

/// Extract main content with custom configuration
pub fn extract_content_with_config(html: &str, config: &ReadabilityConfig) -> String {
    let document = Html::parse_document(html);
    extract_content_from_dom(&document, config)
}

/// Extract main content from a pre-parsed DOM, avoiding redundant parsing
pub fn extract_content_from_dom(document: &Html, config: &ReadabilityConfig) -> String {
    // Try to find the main content container
    if let Some(content) = find_main_content(document, config) {
        return content;
    }

    // Fallback: extract all text from body, filtering out noise
    extract_body_content(document, config)
}

/// Find the main content container
fn find_main_content(document: &Html, config: &ReadabilityConfig) -> Option<String> {
    // Try semantic HTML5 elements first
    let semantic_selectors = [
        "article",
        "main",
        "[role='main']",
        "[role='article']",
        ".article",
        ".post",
        ".content",
        "#content",
        "#main",
        ".entry-content",
        ".post-content",
    ];

    for selector_str in &semantic_selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let content = extract_element_content(&element, config);
                if content.len() > 200 {
                    return Some(content);
                }
            }
        }
    }

    // Score-based approach: find the element with highest content score
    let mut best_element = None;
    let mut best_score = 0.0;

    for element in document.select(div_section_article_selector()) {
        let score = score_element(&element, config);
        if score > best_score {
            best_score = score;
            best_element = Some(element);
        }
    }

    if best_score > 50.0 {
        if let Some(element) = best_element {
            return Some(extract_element_content(&element, config));
        }
    }

    None
}

/// Score an element based on content likelihood
fn score_element(element: &ElementRef, config: &ReadabilityConfig) -> f64 {
    let mut score = 0.0;

    // Get class and id attributes
    let class = element.value().attr("class").unwrap_or("");
    let id = element.value().attr("id").unwrap_or("");
    let combined = format!("{} {}", class, id).to_lowercase();

    // Negative indicators
    for neg in &config.negative_classes {
        if combined.contains(neg) {
            score -= 25.0;
        }
    }

    // Positive indicators
    for pos in &config.positive_classes {
        if combined.contains(pos) {
            score += 25.0;
        }
    }

    // Count paragraphs
    let paragraphs: Vec<_> = element.select(p_selector()).collect();
    score += paragraphs.len() as f64 * 3.0;

    // Count words in paragraphs
    for p in paragraphs {
        let text = p.text().collect::<String>();
        let word_count = text.split_whitespace().count();
        if word_count > 100 {
            score += 10.0;
        } else if word_count > 50 {
            score += 5.0;
        }
    }

    // Penalty for too many links
    let links = element.select(a_selector()).count();
    let skip_tags: &[&str] = &["script", "style", "noscript"];
    let text_len = filtered_text(element, skip_tags).len();
    if text_len > 0 {
        let link_density = links as f64 / (text_len as f64 / 100.0);
        if link_density > 0.5 {
            score -= link_density * 10.0;
        }
    }

    score
}

/// Extract content from an element
fn extract_element_content(element: &ElementRef, config: &ReadabilityConfig) -> String {
    let mut parts = Vec::new();

    // Build list of tags to skip
    let skip_tags: Vec<&str> = config.remove_tags.iter().map(|s| s.as_str()).collect();

    extract_text_recursive(element, &skip_tags, config, &mut parts);

    parts.join("\n\n")
}

/// Collect visible text from an element, skipping unwanted child tags.
///
/// Unlike `element.text().collect()` from the scraper crate (which grabs ALL text
/// nodes regardless of parent tags), this function respects `skip_tags` and won't
/// collect text inside `<script>`, `<style>`, etc.
fn filtered_text(element: &ElementRef, skip_tags: &[&str]) -> String {
    let mut buf = String::new();
    filtered_text_recursive(element, skip_tags, &mut buf);
    buf
}

fn filtered_text_recursive(element: &ElementRef, skip_tags: &[&str], buf: &mut String) {
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            let tag = child_element.value().name();
            if !skip_tags.contains(&tag) {
                filtered_text_recursive(&child_element, skip_tags, buf);
            }
        } else if let Some(text) = child.value().as_text() {
            buf.push_str(text);
        }
    }
}

/// Recursively extract text from element
fn extract_text_recursive(
    element: &ElementRef,
    skip_tags: &[&str],
    config: &ReadabilityConfig,
    parts: &mut Vec<String>,
) {
    let tag_name = element.value().name();

    // Skip unwanted tags
    if skip_tags.contains(&tag_name) {
        return;
    }

    // Check for negative classes
    if let Some(class) = element.value().attr("class") {
        let class_lower = class.to_lowercase();
        for neg in &config.negative_classes {
            if class_lower.contains(neg) {
                return;
            }
        }
    }

    // Handle block-level elements
    match tag_name {
        "p" | "div" | "section" | "article" | "blockquote" | "li" => {
            let text = filtered_text(element, skip_tags);
            let text = text.trim();
            if text.len() >= config.min_paragraph_length {
                parts.push(text.to_string());
            }
        }
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
            let text = filtered_text(element, skip_tags);
            let text = text.trim();
            if !text.is_empty() {
                let level = tag_name.chars().last().unwrap();
                let prefix = "#".repeat(level.to_digit(10).unwrap() as usize);
                parts.push(format!("{} {}", prefix, text));
            }
        }
        "ul" | "ol" => {
            for li in element.select(li_selector()) {
                let text = filtered_text(&li, skip_tags);
                let text = text.trim();
                if !text.is_empty() {
                    parts.push(format!("- {}", text));
                }
            }
        }
        "pre" | "code" => {
            let text = filtered_text(element, skip_tags);
            if !text.trim().is_empty() {
                parts.push(format!("```\n{}\n```", text.trim()));
            }
        }
        _ => {
            // Recurse into children
            for child in element.children() {
                if let Some(child_element) = ElementRef::wrap(child) {
                    extract_text_recursive(&child_element, skip_tags, config, parts);
                }
            }
        }
    }
}

/// Recursively collect raw text from an element, skipping unwanted tags and negative classes.
/// Unlike `extract_text_recursive`, this doesn't structure the output — it just gathers
/// all visible text for the last-resort fallback.
fn collect_filtered_text(
    element: &ElementRef,
    skip_tags: &[&str],
    config: &ReadabilityConfig,
    parts: &mut Vec<String>,
) {
    let tag_name = element.value().name();

    if skip_tags.contains(&tag_name) {
        return;
    }

    if let Some(class) = element.value().attr("class") {
        let class_lower = class.to_lowercase();
        for neg in &config.negative_classes {
            if class_lower.contains(neg) {
                return;
            }
        }
    }

    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            collect_filtered_text(&child_element, skip_tags, config, parts);
        } else if let Some(text) = child.value().as_text() {
            let t = text.trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
    }
}

/// Extract content from body as fallback
fn extract_body_content(document: &Html, config: &ReadabilityConfig) -> String {
    let mut paragraphs = Vec::new();

    // Try to find body
    if let Some(body) = document.select(body_selector()).next() {
        extract_text_recursive(
            &body,
            &config
                .remove_tags
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>(),
            config,
            &mut paragraphs,
        );
    }

    // If structured extraction found nothing, do a raw text walk on <body>
    // but still skip unwanted tags (script, footer, nav, etc.)
    if paragraphs.is_empty() {
        let body = match document.select(body_selector()).next() {
            Some(b) => b,
            None => return String::new(),
        };
        let skip_tags: Vec<&str> = config.remove_tags.iter().map(|s| s.as_str()).collect();
        let mut raw_parts = Vec::new();
        collect_filtered_text(&body, &skip_tags, config, &mut raw_parts);
        let cleaned = raw_parts.join(" ");
        let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
        if cleaned.is_empty() || is_garbage_content(&cleaned) {
            return String::new();
        }
        return cleaned;
    }

    let result = paragraphs.join("\n\n");
    // Final quality check: if the extracted content looks like serialized data, discard it
    if is_garbage_content(&result) {
        return String::new();
    }
    result
}

/// Detect if extracted content is serialized framework data (not human-readable content).
///
/// This catches Next.js RSC payloads (`self.__next_f`), webpack chunks, and similar
/// JavaScript framework serialization artifacts that occasionally slip through
/// as text content.
fn is_garbage_content(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    // Check for Next.js RSC payload patterns
    let garbage_markers = [
        "self.__next_f",
        "__next_f.push",
        "self.__next_data",
        "$Sreact.",
        "\"$undefined\"",
        "static/chunks/",
    ];

    let marker_count = garbage_markers
        .iter()
        .filter(|marker| text.contains(*marker))
        .count();

    // If 2+ markers are found, it's almost certainly RSC garbage
    if marker_count >= 2 {
        return true;
    }

    // Heuristic: if the text has a very high ratio of escaped characters and JSON-like
    // syntax, it's likely serialized data rather than human-readable content.
    // Count backslash-escaped sequences and JSON structural characters.
    let total_chars = text.len();
    if total_chars > 500 {
        let escape_count = text.matches("\\\"").count() + text.matches("\\\\").count();
        let escape_ratio = (escape_count as f64) / (total_chars as f64);
        if escape_ratio > 0.02 {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_article() {
        let html = r#"
            <html>
            <body>
                <nav>Navigation menu</nav>
                <article>
                    <h1>Article Title</h1>
                    <p>This is the first paragraph of the article with enough content to be considered valid text that should be extracted by the readability algorithm.</p>
                    <p>This is the second paragraph with more interesting content about the topic at hand that we want to extract and process.</p>
                </article>
                <footer>Footer content</footer>
            </body>
            </html>
        "#;

        let content = extract_content(html);
        assert!(content.contains("Article Title"));
        assert!(content.contains("first paragraph"));
        assert!(!content.contains("Navigation"));
        assert!(!content.contains("Footer"));
    }

    #[test]
    fn test_extract_main_content() {
        let html = r#"
            <html>
            <body>
                <main>
                    <p>Main content paragraph that is long enough to be considered valid content for extraction purposes.</p>
                </main>
                <aside>Sidebar content</aside>
            </body>
            </html>
        "#;

        let content = extract_content(html);
        assert!(content.contains("Main content"));
        assert!(!content.contains("Sidebar"));
    }

    #[test]
    fn test_code_blocks() {
        let html = r#"
            <html>
            <body>
                <article>
                    <p>Here is some code:</p>
                    <pre><code>fn main() { println!("Hello"); }</code></pre>
                </article>
            </body>
            </html>
        "#;

        let content = extract_content(html);
        // The code content should be extracted (may or may not have backticks depending on extraction path)
        assert!(
            content.contains("fn main()"),
            "Expected 'fn main()' in content: {}",
            content
        );
    }

    #[test]
    fn test_garbage_detection_nextjs_rsc() {
        assert!(is_garbage_content(
            r#"self.__next_f.push([1,"0:{\"P\":null}"])"#
        ));
        assert!(is_garbage_content(
            r#"(self.__next_f=self.__next_f||[]).push([0]) self.__next_f.push([1,"$Sreact.fragment"])"#
        ));
    }

    #[test]
    fn test_garbage_detection_normal_content() {
        assert!(!is_garbage_content(
            "This is a normal paragraph about Meilisearch search engine."
        ));
        assert!(!is_garbage_content(""));
    }

    #[test]
    fn test_rsc_inside_content_container() {
        // Regression test: script tags inside a content container were leaking
        // through because element.text().collect() ignores tag boundaries.
        let html = r#"
            <html>
            <body>
                <main>
                    <p>This is actual page content that should be long enough to be extracted by readability.</p>
                    <script>(self.__next_f=self.__next_f||[]).push([0])</script>
                    <script>self.__next_f.push([1,"$Sreact.fragment\n\"some\":\"json\""])</script>
                    <p>Another paragraph with real content about how Meilisearch works with search.</p>
                </main>
            </body>
            </html>
        "#;
        let content = extract_content(html);
        assert!(
            !content.contains("__next_f"),
            "RSC payload should not appear in content: {}",
            content
        );
        assert!(content.contains("actual page content"));
        assert!(content.contains("Another paragraph"));
    }

    #[test]
    fn test_nextjs_rsc_page_returns_empty() {
        let html = r#"
            <html>
            <body>
                <script>(self.__next_f=self.__next_f||[]).push([0])</script>
                <script>self.__next_f.push([1,"$Sreact.fragment"])</script>
            </body>
            </html>
        "#;
        let content = extract_content(html);
        // Script tags are removed, so if there's no other content, result should be empty
        assert!(
            content.is_empty() || !content.contains("__next_f"),
            "RSC payload should not appear in content: {}",
            content
        );
    }
}
