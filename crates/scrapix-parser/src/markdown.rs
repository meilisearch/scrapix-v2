//! HTML to Markdown conversion

use html2md::parse_html;
use regex::Regex;
use std::sync::OnceLock;

/// Configuration for markdown conversion
#[derive(Debug, Clone)]
pub struct MarkdownConfig {
    /// Maximum line length for wrapping
    pub max_line_length: Option<usize>,
    /// Whether to preserve link titles
    pub preserve_link_titles: bool,
    /// Whether to convert images to markdown
    pub convert_images: bool,
    /// Whether to convert code blocks
    pub convert_code_blocks: bool,
    /// Whether to strip non-content sections (nav, footer, header, aside)
    pub only_main_content: bool,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            max_line_length: None,
            preserve_link_titles: true,
            convert_images: true,
            convert_code_blocks: true,
            only_main_content: false,
        }
    }
}

/// Convert HTML to Markdown (full page)
pub fn html_to_markdown(html: &str) -> String {
    html_to_markdown_with_config(html, &MarkdownConfig::default())
}

/// Convert HTML to clean Markdown, stripping boilerplate (nav, footer, etc.)
pub fn html_to_main_content_markdown(html: &str) -> String {
    let mut config = MarkdownConfig::default();
    config.only_main_content = true;
    html_to_markdown_with_config(html, &config)
}

/// Convert HTML to Markdown with custom configuration
pub fn html_to_markdown_with_config(html: &str, config: &MarkdownConfig) -> String {
    // Strip non-content tags (script, style, etc.)
    let mut cleaned_html = strip_non_content_tags(html);

    // Optionally strip boilerplate sections
    if config.only_main_content {
        cleaned_html = strip_boilerplate_tags(&cleaned_html);
    }

    // Use html2md for the basic conversion
    let markdown = parse_html(&cleaned_html);

    // Strip any remaining HTML tags that html2md didn't convert
    let stripped = strip_remaining_html(&markdown);

    // Post-process the markdown
    clean_markdown(&stripped)
}

/// Strip script, style, noscript, svg, and head tags (and their content) from HTML.
fn strip_non_content_tags(html: &str) -> String {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        ["script", "style", "noscript", "svg", "head"]
            .iter()
            .map(|tag| {
                Regex::new(&format!(r"(?si)<{tag}[\s>].*?</{tag}\s*>")).unwrap()
            })
            .collect()
    });

    let mut result = html.to_string();
    for re in patterns {
        result = re.replace_all(&result, "").into_owned();
    }
    result
}

/// Strip boilerplate HTML sections: nav, header, footer, aside.
fn strip_boilerplate_tags(html: &str) -> String {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    let patterns = PATTERNS.get_or_init(|| {
        ["nav", "footer", "header", "aside"]
            .iter()
            .map(|tag| {
                Regex::new(&format!(r"(?si)<{tag}[\s>].*?</{tag}\s*>")).unwrap()
            })
            .collect()
    });

    let mut result = html.to_string();
    for re in patterns {
        result = re.replace_all(&result, "").into_owned();
    }
    result
}

/// Strip any remaining HTML tags from markdown output.
///
/// Converts `<img>` tags to `![alt](src)` before stripping, so image info is preserved.
/// All other HTML tags are removed, keeping only their text content.
fn strip_remaining_html(markdown: &str) -> String {
    // Convert <img> tags to markdown images: ![alt](src)
    static IMG_RE: OnceLock<Regex> = OnceLock::new();
    let img_re = IMG_RE.get_or_init(|| Regex::new(r#"(?i)<img\s[^>]*?>"#).unwrap());

    static ALT_RE: OnceLock<Regex> = OnceLock::new();
    let alt_re = ALT_RE.get_or_init(|| Regex::new(r#"(?i)alt\s*=\s*"([^"]*)""#).unwrap());

    static SRC_RE: OnceLock<Regex> = OnceLock::new();
    let src_re = SRC_RE.get_or_init(|| Regex::new(r#"(?i)src\s*=\s*"([^"]*)""#).unwrap());

    let result = img_re.replace_all(markdown, |caps: &regex::Captures| {
        let tag = &caps[0];
        let alt = alt_re
            .captures(tag)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let src = src_re
            .captures(tag)
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        if alt.is_empty() && src.is_empty() {
            String::new()
        } else {
            format!("![{alt}]({src})")
        }
    });

    // Strip all remaining HTML tags (opening, closing, self-closing)
    static TAG_RE: OnceLock<Regex> = OnceLock::new();
    let tag_re = TAG_RE.get_or_init(|| Regex::new(r"<[^>]+>").unwrap());

    tag_re.replace_all(&result, "").into_owned()
}

/// Clean up the markdown output
fn clean_markdown(markdown: &str) -> String {
    // First pass: clean up multiline link/image syntax
    // [\n\nText\n\n](url) → [Text](url)
    // ![\n\nAlt\n\n](url) → ![Alt](url)
    static MULTILINE_LINK_RE: OnceLock<Regex> = OnceLock::new();
    let multiline_link_re = MULTILINE_LINK_RE.get_or_init(|| {
        Regex::new(r"(!?\[)\s*\n\s*(.*?)\s*\n\s*\]\(([^)]*)\)").unwrap()
    });
    let markdown = multiline_link_re.replace_all(markdown, "$1$2]($3)");

    // Remove empty links: [](url)
    static EMPTY_LINK_RE: OnceLock<Regex> = OnceLock::new();
    let empty_link_re =
        EMPTY_LINK_RE.get_or_init(|| Regex::new(r"\[]\([^)]*\)").unwrap());
    let markdown = empty_link_re.replace_all(&markdown, "");

    // Remove images with relative/asset URLs (decorative)
    static DECORATIVE_IMG_RE: OnceLock<Regex> = OnceLock::new();
    let decorative_img_re = DECORATIVE_IMG_RE.get_or_init(|| {
        Regex::new(r"!\[[^\]]*\]\(/_next/[^)]*\)").unwrap()
    });
    let markdown = decorative_img_re.replace_all(&markdown, "");

    // Remove images with no alt text
    static NO_ALT_IMG_RE: OnceLock<Regex> = OnceLock::new();
    let no_alt_img_re =
        NO_ALT_IMG_RE.get_or_init(|| Regex::new(r"!\[\]\([^)]*\)").unwrap());
    let markdown = no_alt_img_re.replace_all(&markdown, "");

    // Second pass: line-by-line cleanup
    let mut result = String::new();
    let mut prev_blank = false;
    let mut in_code_block = false;

    for line in markdown.lines() {
        // Track code blocks
        if line.starts_with("```") {
            in_code_block = !in_code_block;
        }

        // In code blocks, preserve everything
        if in_code_block {
            result.push_str(line);
            result.push('\n');
            prev_blank = false;
            continue;
        }

        let trimmed = line.trim();

        // Skip multiple blank lines
        if trimmed.is_empty() {
            if !prev_blank {
                result.push('\n');
                prev_blank = true;
            }
            continue;
        }

        prev_blank = false;

        // Clean up the line
        let cleaned = clean_line(trimmed);
        if !cleaned.is_empty() {
            result.push_str(&cleaned);
            result.push('\n');
        }
    }

    // Trim trailing whitespace
    result.trim_end().to_string()
}

/// Clean a single line of markdown
fn clean_line(line: &str) -> String {
    // Collapse consecutive spaces in a single pass - O(n) with one allocation
    let mut result = String::with_capacity(line.len());
    let mut prev_space = false;
    for ch in line.chars() {
        if ch == ' ' {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }

    // Fix heading spacing
    if result.starts_with('#') {
        let hash_end = result.find(|c: char| c != '#').unwrap_or(result.len());
        if hash_end < result.len() && !result[hash_end..].starts_with(' ') {
            result.insert(hash_end, ' ');
        }
    }

    result
}

/// Extract plain text from markdown (strip all formatting)
pub fn markdown_to_text(markdown: &str) -> String {
    let mut result = String::new();
    let mut in_code_block = false;

    for line in markdown.lines() {
        // Track code blocks
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if in_code_block {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        let mut cleaned = line.to_string();

        // Remove heading markers
        while cleaned.starts_with('#') {
            cleaned = cleaned.trim_start_matches('#').to_string();
        }

        // Remove emphasis markers
        cleaned = cleaned
            .replace("**", "")
            .replace("__", "")
            .replace(['*', '_'], "");

        // Remove link syntax but keep text
        // [text](url) -> text
        while let Some(start) = cleaned.find('[') {
            if let Some(end) = cleaned[start..].find("](") {
                let link_end = cleaned[start + end..].find(')');
                if let Some(link_end) = link_end {
                    let text = &cleaned[start + 1..start + end];
                    cleaned = format!(
                        "{}{}{}",
                        &cleaned[..start],
                        text,
                        &cleaned[start + end + link_end + 1..]
                    );
                    continue;
                }
            }
            break;
        }

        // Remove image syntax
        cleaned = cleaned.replace("![", "[").replace("](", " ");

        // Remove list markers
        if cleaned.starts_with("- ") || cleaned.starts_with("* ") {
            cleaned = cleaned[2..].to_string();
        }

        // Remove numbered list markers
        if let Some(pos) = cleaned.find(". ") {
            if cleaned[..pos].chars().all(|c| c.is_ascii_digit()) {
                cleaned = cleaned[pos + 2..].to_string();
            }
        }

        let trimmed = cleaned.trim();
        if !trimmed.is_empty() {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(trimmed);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_html_to_markdown() {
        let html = "<h1>Title</h1><p>Paragraph text</p>";
        let md = html_to_markdown(html);

        // html2md may format headings differently, check for title presence
        assert!(md.contains("Title"), "Expected 'Title' in markdown: {}", md);
        assert!(md.contains("Paragraph text"));
    }

    #[test]
    fn test_links() {
        let html = r#"<a href="https://example.com">Link text</a>"#;
        let md = html_to_markdown(html);

        assert!(md.contains("[Link text]"));
        assert!(md.contains("https://example.com"));
    }

    #[test]
    fn test_lists() {
        let html = "<ul><li>Item 1</li><li>Item 2</li></ul>";
        let md = html_to_markdown(html);

        assert!(md.contains("Item 1"));
        assert!(md.contains("Item 2"));
    }

    #[test]
    fn test_code_blocks() {
        let html = "<pre><code>fn main() {}</code></pre>";
        let md = html_to_markdown(html);

        assert!(md.contains("fn main()"));
    }

    #[test]
    fn test_strips_nav_footer_in_main_content_mode() {
        let html = r#"
            <nav><a href="/">Home</a><a href="/about">About</a></nav>
            <main><h1>Article</h1><p>Content here</p></main>
            <footer><p>Copyright 2026</p></footer>
        "#;
        let md = html_to_main_content_markdown(html);

        assert!(md.contains("Article"));
        assert!(md.contains("Content here"));
        assert!(!md.contains("Home"));
        assert!(!md.contains("Copyright"));
    }

    #[test]
    fn test_strips_decorative_images() {
        let html = r#"
            <img src="/_next/static/media/bg.svg" alt="">
            <h1>Title</h1>
            <img src="https://example.com/photo.jpg" alt="A photo">
        "#;
        let md = html_to_main_content_markdown(html);

        assert!(md.contains("Title"));
        assert!(!md.contains("/_next/"));
        assert!(md.contains("![A photo](https://example.com/photo.jpg)"));
    }

    #[test]
    fn test_removes_empty_links() {
        let html = r#"
            <a href="https://github.com"></a>
            <a href="https://example.com">Real link</a>
        "#;
        let md = html_to_markdown(html);

        assert!(md.contains("[Real link]"));
        assert!(!md.contains("[]("));
    }

    #[test]
    fn test_markdown_to_text() {
        let md = "# Title\n\nSome **bold** and *italic* text.\n\n[Link](http://example.com)";
        let text = markdown_to_text(md);

        assert!(text.contains("Title"));
        assert!(text.contains("bold"));
        assert!(text.contains("italic"));
        assert!(text.contains("Link"));
        assert!(!text.contains("**"));
        assert!(!text.contains("http://"));
    }

    #[test]
    fn test_clean_multiple_blanks() {
        let md = "Line 1\n\n\n\n\nLine 2";
        let cleaned = clean_markdown(md);

        // Should have at most one blank line between content
        assert!(!cleaned.contains("\n\n\n"));
    }
}
