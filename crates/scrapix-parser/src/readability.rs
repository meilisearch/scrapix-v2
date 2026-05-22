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

/// Check whether any of `needles` appears as a discrete *word* inside the
/// element's `class` / `id` attribute. Used by the readability extractor to
/// decide if a node looks like navigation, sidebar, footer, etc.
///
/// **Why this is its own helper, and why it is NOT a substring match.**
/// Tailwind and other utility CSS frameworks use class names whose internal
/// segments happen to contain English words that are unrelated to the
/// semantics we care about. For example:
///   - `prose-headings:scroll-mt-24`  contains "ad" inside "headings"
///   - `pt-[var(--header-height)]`    contains "header" inside a CSS variable
///   - `leading-relaxed`              contains "ad" inside "leading"
///
/// A naïve `class.contains("ad")` flags ALL of those as advertisements, and
/// the result is that fully legitimate article bodies get discarded by the
/// extractor. (Observed on meilisearch.com's blog: 217 of 315 posts produced
/// zero indexable content because of this.)
///
/// We solve it by splitting class names on any character that is NOT
/// alphanumeric AND not inside parentheses / square brackets (which is how
/// Tailwind encodes arbitrary values that should be treated as opaque), then
/// requiring **equality** with one of the needles. So:
///   - `sidebar`           tokens: ["sidebar"]                    → matches "sidebar"
///   - `main-nav`          tokens: ["main", "nav"]                → matches "nav"
///   - `nav-bar`           tokens: ["nav", "bar"]                 → matches "nav"
///   - `site-header`       tokens: ["site", "header"]             → matches "header"
///   - `prose-headings`    tokens: ["prose", "headings"]          → no match for "ad"
///   - `pt-[var(--h-h)]`   skipped (contains `[`)                 → no false positive
///   - `prose-p:text-...`  tokens: ["prose","p","text",...]       → no match for "ad"
///
/// The id is checked the same way; ids tend to be simple identifiers without
/// utility-class noise, so this just generalizes nicely.
///
/// All comparisons are case-insensitive; needles are assumed to be lowercase
/// (which matches how `ReadabilityConfig::default()` populates them).
fn class_id_matches_any(class: &str, id: &str, needles: &[String]) -> bool {
    for value in [class, id] {
        for token in value.split_whitespace() {
            // Tailwind arbitrary values: `pt-[var(--header-height)]`,
            // `bg-[#fff]`, `grid-cols-[1fr_2fr]`, etc. These are opaque to us;
            // never treat their interior as semantic class tokens.
            if token.contains('[') || token.contains('(') {
                continue;
            }
            let lower = token.to_ascii_lowercase();
            for segment in lower.split(|c: char| !c.is_ascii_alphanumeric()) {
                if segment.is_empty() {
                    continue;
                }
                if needles.iter().any(|n| n == segment) {
                    return true;
                }
            }
        }
    }
    false
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

    // Negative indicators (word-boundary match — see class_id_matches_any).
    if class_id_matches_any(class, id, &config.negative_classes) {
        score -= 25.0;
    }

    // Positive indicators (same matching rules so the score is symmetrical).
    if class_id_matches_any(class, id, &config.positive_classes) {
        score += 25.0;
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

/// HTML block-level elements per CSS default `display: block`.
///
/// Used to decide where paragraph boundaries belong during text extraction.
/// On minified HTML (most modern SPAs), there is no whitespace between
/// adjacent block tags, so we must inject the boundary ourselves — otherwise
/// the words on either side run together (`<h1>Blog</h1><p>Tutorials</p>`
/// → `BlogTutorials`).
fn is_block_element(tag: &str) -> bool {
    matches!(
        tag,
        "address"
            | "article"
            | "aside"
            | "blockquote"
            | "dd"
            | "details"
            | "dialog"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "hgroup"
            | "hr"
            | "li"
            | "main"
            | "nav"
            | "ol"
            | "p"
            | "pre"
            | "section"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "tr"
            | "ul"
    )
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

    // Check for negative classes (word-boundary match — see class_id_matches_any).
    let class = element.value().attr("class").unwrap_or("");
    let id = element.value().attr("id").unwrap_or("");
    if class_id_matches_any(class, id, &config.negative_classes) {
        return;
    }

    // Handle block-level elements
    match tag_name {
        // Paragraph blocks: their children are inline content by HTML spec,
        // so the whole subtree is one paragraph. Apply min_paragraph_length
        // to filter boilerplate.
        "p" | "blockquote" => {
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
        // Container blocks and everything else (body, main, table, span at
        // the top of a recursion, etc.): walk the children. Block children
        // recurse so each becomes its own paragraph; runs of inline content
        // between blocks accumulate into a single paragraph and are flushed
        // at each block boundary.
        _ => {
            process_mixed_children(element, skip_tags, config, parts);
        }
    }
}

/// Walk an element's children, splitting on block-element boundaries.
///
/// Block descendants recurse into `extract_text_recursive`. Consecutive
/// inline content (and raw text nodes) accumulate into a buffer that is
/// flushed as a single paragraph whenever a block boundary is crossed (or
/// at the end of the parent). The walk descends through inline wrappers
/// without flushing, because HTML5 allows inline elements like `<a>` to
/// contain block-level children — a flat sweep is required to find them.
fn process_mixed_children(
    element: &ElementRef,
    skip_tags: &[&str],
    config: &ReadabilityConfig,
    parts: &mut Vec<String>,
) {
    let mut inline_buf = String::new();
    walk_mixed_tree(element, skip_tags, config, parts, &mut inline_buf);
    flush_inline_paragraph(&mut inline_buf, parts);
}

fn walk_mixed_tree(
    element: &ElementRef,
    skip_tags: &[&str],
    config: &ReadabilityConfig,
    parts: &mut Vec<String>,
    inline_buf: &mut String,
) {
    for child in element.children() {
        if let Some(child_element) = ElementRef::wrap(child) {
            let tag = child_element.value().name();
            if skip_tags.contains(&tag) {
                continue;
            }
            // Honor negative-class filtering on inline elements too — keeps
            // share/social/comment widgets out of the accumulated paragraph.
            // Word-boundary match (see class_id_matches_any) so Tailwind
            // utility classes like `prose-headings` don't get flagged as "ad".
            let class = child_element.value().attr("class").unwrap_or("");
            let id = child_element.value().attr("id").unwrap_or("");
            if class_id_matches_any(class, id, &config.negative_classes) {
                continue;
            }

            if is_block_element(tag) {
                flush_inline_paragraph(inline_buf, parts);
                extract_text_recursive(&child_element, skip_tags, config, parts);
            } else {
                // Inline wrapper — its subtree may still contain blocks, so
                // keep walking with the same buffer. Inline text flows into
                // `inline_buf`; any deeper block triggers a flush.
                walk_mixed_tree(&child_element, skip_tags, config, parts, inline_buf);
            }
        } else if let Some(text) = child.value().as_text() {
            inline_buf.push_str(text);
        }
    }
}

fn flush_inline_paragraph(buf: &mut String, parts: &mut Vec<String>) {
    let trimmed = buf.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    buf.clear();
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

    // Word-boundary class/id match (see class_id_matches_any).
    let class = element.value().attr("class").unwrap_or("");
    let id = element.value().attr("id").unwrap_or("");
    if class_id_matches_any(class, id, &config.negative_classes) {
        return;
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
    fn test_address_not_penalized_as_ad() {
        // Regression test for the substring-match bug: `address` used to match
        // negative class `ad` and silently drop legitimate content.
        let html = r#"
            <html>
            <body>
                <div class="address">
                    <p>This is the company address section with plenty of content that should be extracted normally without penalization from the readability algorithm.</p>
                    <p>123 Main Street, City, State 12345. The address information is important content.</p>
                </div>
            </body>
            </html>
        "#;
        let content = extract_content(html);
        assert!(
            content.contains("company address section"),
            "`address` class must not be treated as `ad`: got {:?}",
            content
        );
    }

    #[test]
    fn test_tailwind_prose_classes_not_penalized() {
        // Regression: meilisearch.com (and many Tailwind-based sites) put the
        // article body inside a `<article class="prose prose-headings:scroll-mt-24 …">`.
        // The old substring-match incorrectly flagged this as a `header`/`ad`
        // class and silently produced zero content — see investigation in
        // crates/scrapix-parser/src/readability.rs::class_id_matches_any.
        let html = r#"
            <html>
            <body>
                <main class="flex-1 pt-[var(--header-height)]">
                    <article class="prose prose-lg prose-invert max-w-none prose-headings:scroll-mt-24 prose-headings:text-white prose-p:text-translucent-white-700 prose-a:text-brand-pink">
                        <h1 class="my-4 leading-relaxed">Real article title</h1>
                        <p class="my-4 leading-relaxed">This is a paragraph of real article body content that absolutely should reach the search index. It must be long enough to clear the min_paragraph_length filter and survive the readability scoring.</p>
                        <p class="my-4 leading-relaxed">Another paragraph that further demonstrates the body is real, with enough words to bias scoring positively in any case. Lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt.</p>
                    </article>
                </main>
            </body>
            </html>
        "#;
        let content = extract_content(html);
        assert!(
            content.contains("Real article title"),
            "Tailwind `prose-headings:*` must not be treated as a `header` element: got {:?}",
            content
        );
        assert!(
            content.contains("real article body content"),
            "Tailwind `prose-p` paragraphs must not be treated as `ad`: got {:?}",
            content
        );
        assert!(
            content.contains("Another paragraph"),
            "Tailwind `leading-relaxed` paragraphs must not be treated as `ad`: got {:?}",
            content
        );
    }

    #[test]
    fn test_class_id_matches_any_word_boundary() {
        let negs = vec![
            "ad".to_string(),
            "header".to_string(),
            "nav".to_string(),
            "sidebar".to_string(),
        ];
        // True positives — actual semantic uses of negative class names.
        assert!(class_id_matches_any("ad-banner", "", &negs));
        assert!(class_id_matches_any("page-header", "", &negs));
        assert!(class_id_matches_any("main-nav", "", &negs));
        assert!(class_id_matches_any("nav-bar", "", &negs));
        assert!(class_id_matches_any("sidebar", "", &negs));
        assert!(class_id_matches_any("foo bar sidebar baz", "", &negs));
        assert!(class_id_matches_any("", "main-header", &negs));
        // False positives that the old contains()-based matcher produced.
        assert!(!class_id_matches_any("address", "", &negs));
        assert!(!class_id_matches_any("prose-headings", "", &negs));
        assert!(!class_id_matches_any("leading-relaxed", "", &negs));
        assert!(!class_id_matches_any("add-product", "", &negs));
        assert!(!class_id_matches_any("navigate-back", "", &negs)); // "navigate" != "nav"
                                                                    // Tailwind arbitrary values: contents of `[…]` and `(…)` are opaque.
        assert!(!class_id_matches_any(
            "pt-[var(--header-height)]",
            "",
            &negs
        ));
        assert!(!class_id_matches_any("bg-[#ad0000]", "", &negs));
        assert!(!class_id_matches_any("grid-cols-[1fr_2fr]", "", &negs));
        // Empty input never matches.
        assert!(!class_id_matches_any("", "", &negs));
    }

    #[test]
    fn test_actual_ad_div_penalized() {
        let html = r#"
            <html>
            <body>
                <article>
                    <p>This is the main article content with enough text to be properly extracted by the readability algorithm in this test case.</p>
                </article>
                <div class="ad-banner">
                    <p>Buy our product now! This amazing deal with lots of text should not appear in extracted content at all.</p>
                </div>
            </body>
            </html>
        "#;
        let content = extract_content(html);
        assert!(content.contains("main article content"));
        assert!(!content.contains("Buy our product"));
    }

    #[test]
    fn test_prefers_larger_semantic_element() {
        // Documents design issue: first semantic match with >200 chars wins,
        // even if a later element has much more content.
        let short_article = "A".repeat(201);
        let long_main = "B".repeat(5000);
        let html = format!(
            r#"
            <html>
            <body>
                <article><p>{}</p></article>
                <main><p>{}</p></main>
            </body>
            </html>
        "#,
            short_article, long_main
        );
        let content = extract_content(&html);
        // Known issue: returns article content (first match) even though main is much larger
        // This documents the first-match behavior
        assert!(
            content.contains(&"A".repeat(50)),
            "Should extract some content"
        );
    }

    #[test]
    fn test_minified_block_boundaries_do_not_join_words() {
        // Regression: on minified HTML (no whitespace text nodes between
        // adjacent tags — the norm for React/Next.js apps) adjacent block
        // elements used to produce text like "BlogTutorials" because the
        // extractor collapsed the whole div subtree without a separator.
        let html = r#"<html><body><article><div><h1>Blog</h1><p>Tutorials, product updates, and insights from the Meilisearch team about modern search.</p></div></article></body></html>"#;
        let content = extract_content(html);
        assert!(
            !content.contains("BlogTutorials"),
            "h1 and following p must not join into one word. Got: {}",
            content
        );
        assert!(content.contains("Blog"));
        assert!(content.contains("Tutorials"));
    }

    #[test]
    fn test_sibling_blocks_become_separate_paragraphs() {
        // Each block-level sibling (heading + body) should be its own
        // paragraph rather than one collapsed string.
        let html = r#"<html><body><main><div><h2>RAG for structured data</h2><p>Discover how RAG for structured data improves AI accuracy across applications.</p></div><div><h2>RAG reranking explained</h2><p>Learn what reranking is and why it matters for retrieval quality at scale.</p></div></main></body></html>"#;
        let content = extract_content(html);
        assert!(
            !content.contains("dataDiscover"),
            "First card's heading should not join with its body. Got: {}",
            content
        );
        assert!(
            !content.contains("applicationsRAG"),
            "First card body should not join with second card heading. Got: {}",
            content
        );
        assert!(
            !content.contains("explainedLearn"),
            "Second card's heading should not join with its body. Got: {}",
            content
        );
    }

    #[test]
    fn test_meilisearch_blog_shape_no_joined_words() {
        // Shape modelled on www.meilisearch.com/blog, where the user reported
        // output like `BlogTutorials...moreDiscover how...Maya ShinMay 14`.
        // The card uses inline siblings for author/date, so those will still
        // concatenate without whitespace (browser does the same when CSS gap
        // creates the visual separation), but every block boundary must be
        // preserved.
        let html = r##"<html><body><main><div><h1>Blog</h1><p>Tutorials, product updates, and insights from the Meilisearch team</p></div><article><a href="#"><h2>RAG for structured data: benefits, challenges, examples, &amp; more</h2><p>Discover how RAG for structured data improves AI accuracy and how to implement it effectively.</p></a></article></main></body></html>"##;
        let content = extract_content(html);

        for joined in ["BlogTutorials", "moreDiscover"] {
            assert!(
                !content.contains(joined),
                "block boundary should split words but found {:?} in: {}",
                joined,
                content
            );
        }
        assert!(content.contains("Blog"));
        assert!(content.contains("Tutorials"));
        assert!(content.contains("Discover how RAG"));
    }

    #[test]
    fn test_div_with_only_inline_text_kept_as_paragraph() {
        // A `<div>` that contains only inline content (text + anchor) should
        // still produce a paragraph — option 2 must not lose plain inline
        // divs in the process of separating block boundaries.
        let html = r##"<html><body><article><div>This is body copy with an <a href="#">inline link</a> embedded inside a div that contains only inline content.</div></article></body></html>"##;
        let content = extract_content(html);
        assert!(
            content.contains("inline link"),
            "Inline anchor text should be preserved. Got: {}",
            content
        );
        assert!(
            content.contains("only inline content"),
            "Inline div text should be preserved. Got: {}",
            content
        );
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
