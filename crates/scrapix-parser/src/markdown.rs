//! HTML to Markdown conversion

use html2md::parse_html;

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
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            max_line_length: None,
            preserve_link_titles: true,
            convert_images: true,
            convert_code_blocks: true,
        }
    }
}

/// Convert HTML to Markdown
pub fn html_to_markdown(html: &str) -> String {
    html_to_markdown_with_config(html, &MarkdownConfig::default())
}

/// Convert HTML to Markdown with custom configuration
pub fn html_to_markdown_with_config(html: &str, _config: &MarkdownConfig) -> String {
    // Use html2md for the basic conversion
    let markdown = parse_html(html);

    // Post-process the markdown
    clean_markdown(&markdown)
}

/// Clean up the markdown output
fn clean_markdown(markdown: &str) -> String {
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
    let mut result = line.to_string();

    // Remove excessive whitespace
    while result.contains("  ") {
        result = result.replace("  ", " ");
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
