//! MiniHTML conversion — strips HTML to semantic tags with minimal attributes.

use scraper::node::Node;
use scraper::Html;

/// Tags whose content is kept as-is (with allowed attributes).
const KEPT_TAGS: &[&str] = &[
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "p",
    "blockquote",
    "hr",
    "br",
    "a",
    "img",
    "ul",
    "ol",
    "li",
    "pre",
    "code",
    "table",
    "thead",
    "tbody",
    "tr",
    "td",
    "th",
    "strong",
    "em",
    "b",
    "i",
];

/// Tags stripped entirely (tag + all descendants).
const STRIPPED_TAGS: &[&str] = &["script", "style", "noscript", "svg", "head"];

/// Boilerplate tags stripped when `only_main_content` is true.
const BOILERPLATE_TAGS: &[&str] = &["nav", "footer", "header", "aside"];

/// Attributes allowed per tag. Tags not listed here keep no attributes.
fn allowed_attrs(tag: &str) -> &'static [&'static str] {
    match tag {
        "a" => &["href"],
        "img" => &["src", "alt"],
        _ => &[],
    }
}

/// Convert full-page HTML to MiniHTML (all semantic content preserved).
pub fn html_to_minihtml(html: &str) -> String {
    let dom = Html::parse_document(html);
    let mut out = String::with_capacity(html.len() / 2);
    for child in dom.tree.root().children() {
        walk_node(&child, false, &mut out);
    }
    collapse_whitespace(&out)
}

/// Convert HTML to MiniHTML, stripping boilerplate sections (nav, footer, header, aside).
pub fn html_to_main_content_minihtml(html: &str) -> String {
    let dom = Html::parse_document(html);
    let mut out = String::with_capacity(html.len() / 2);
    for child in dom.tree.root().children() {
        walk_node(&child, true, &mut out);
    }
    collapse_whitespace(&out)
}

fn walk_node(node: &ego_tree::NodeRef<Node>, strip_boilerplate: bool, out: &mut String) {
    match node.value() {
        Node::Text(text) => {
            escape_html_to(text, out);
        }
        Node::Element(el) => {
            let tag = el.name();

            // Strip entirely — skip tag and all children
            if STRIPPED_TAGS.contains(&tag) {
                return;
            }
            if strip_boilerplate && BOILERPLATE_TAGS.contains(&tag) {
                return;
            }

            let is_kept = KEPT_TAGS.contains(&tag);

            if is_kept {
                out.push('<');
                out.push_str(tag);
                for attr_name in allowed_attrs(tag) {
                    if let Some(val) = el.attr(attr_name) {
                        out.push(' ');
                        out.push_str(attr_name);
                        out.push_str("=\"");
                        escape_html_to(val, out);
                        out.push('"');
                    }
                }
                // Self-closing tags
                if matches!(tag, "hr" | "br" | "img") {
                    out.push_str(" />");
                    return;
                }
                out.push('>');
            }

            // Recurse into children
            for child in node.children() {
                walk_node(&child, strip_boilerplate, out);
            }

            if is_kept {
                out.push_str("</");
                out.push_str(tag);
                out.push('>');
            }
        }
        Node::Document => {
            for child in node.children() {
                walk_node(&child, strip_boilerplate, out);
            }
        }
        _ => {}
    }
}

fn escape_html_to(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// Collapse runs of whitespace into single spaces and trim around block tags.
fn collapse_whitespace(html: &str) -> String {
    // First pass: collapse whitespace-only text runs
    let mut result = String::with_capacity(html.len());
    let mut prev_was_space = false;
    let mut chars = html.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            // Copy the tag verbatim
            prev_was_space = false;
            result.push('<');
            while let Some(&next) = chars.peek() {
                chars.next();
                result.push(next);
                if next == '>' {
                    break;
                }
            }
        } else if ch.is_ascii_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            result.push(ch);
            prev_was_space = false;
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_minihtml() {
        let html = "<html><body><h1>Title</h1><p>Hello <strong>world</strong></p></body></html>";
        let mini = html_to_minihtml(html);
        assert_eq!(mini, "<h1>Title</h1><p>Hello <strong>world</strong></p>");
    }

    #[test]
    fn test_strips_scripts_and_styles() {
        let html = r#"<html><head><style>body{color:red}</style></head><body>
            <script>alert('xss')</script>
            <p>Safe content</p>
        </body></html>"#;
        let mini = html_to_minihtml(html);
        assert!(mini.contains("<p>Safe content</p>"));
        assert!(!mini.contains("script"));
        assert!(!mini.contains("style"));
        assert!(!mini.contains("alert"));
    }

    #[test]
    fn test_unwraps_divs_and_spans() {
        let html = "<div><span>Text in <em>spans</em></span></div>";
        let mini = html_to_minihtml(html);
        assert_eq!(mini, "Text in <em>spans</em>");
    }

    #[test]
    fn test_preserves_links_with_href_only() {
        let html = r#"<a href="https://example.com" class="btn" id="link1">Click</a>"#;
        let mini = html_to_minihtml(html);
        assert_eq!(mini, r#"<a href="https://example.com">Click</a>"#);
    }

    #[test]
    fn test_preserves_images_with_src_alt_only() {
        let html = r#"<img src="photo.jpg" alt="A photo" class="hero" width="100">"#;
        let mini = html_to_minihtml(html);
        assert_eq!(mini, r#"<img src="photo.jpg" alt="A photo" />"#);
    }

    #[test]
    fn test_strips_boilerplate_in_main_content_mode() {
        let html = r#"<html><body>
            <nav><a href="/">Home</a></nav>
            <main><h1>Article</h1><p>Content</p></main>
            <footer><p>Copyright</p></footer>
        </body></html>"#;
        let mini = html_to_main_content_minihtml(html);
        assert!(mini.contains("<h1>Article</h1>"));
        assert!(mini.contains("<p>Content</p>"));
        assert!(!mini.contains("Home"));
        assert!(!mini.contains("Copyright"));
    }

    #[test]
    fn test_keeps_boilerplate_in_full_mode() {
        let html = "<nav><a href=\"/\">Home</a></nav><p>Content</p>";
        let mini = html_to_minihtml(html);
        assert!(mini.contains("Home"));
        assert!(mini.contains("Content"));
    }

    #[test]
    fn test_table_structure() {
        let html = "<table><thead><tr><th>Col</th></tr></thead><tbody><tr><td>Val</td></tr></tbody></table>";
        let mini = html_to_minihtml(html);
        assert_eq!(mini, html);
    }

    #[test]
    fn test_escapes_special_chars() {
        let html = "<p>1 &lt; 2 &amp; 3 &gt; 0</p>";
        let mini = html_to_minihtml(html);
        assert!(mini.contains("1 &lt; 2 &amp; 3 &gt; 0"));
    }

    #[test]
    fn test_hr_and_br_self_closing() {
        let html = "<p>Line 1<br>Line 2</p><hr><p>After</p>";
        let mini = html_to_minihtml(html);
        assert!(mini.contains("<br />"));
        assert!(mini.contains("<hr />"));
    }

    #[test]
    fn test_collapses_whitespace() {
        let html = "<p>  Multiple   spaces   here  </p>";
        let mini = html_to_minihtml(html);
        assert_eq!(mini, "<p> Multiple spaces here </p>");
    }

    #[test]
    fn test_lists() {
        let html = "<ul><li>One</li><li>Two</li></ul><ol><li>A</li><li>B</li></ol>";
        let mini = html_to_minihtml(html);
        assert_eq!(mini, html);
    }

    #[test]
    fn test_pre_code_preserved() {
        let html = "<pre><code>fn main() {}</code></pre>";
        let mini = html_to_minihtml(html);
        assert_eq!(mini, html);
    }

    #[test]
    fn test_noscript_stripped() {
        let html = "<p>Hello</p><noscript><p>Enable JS</p></noscript><p>World</p>";
        let mini = html_to_minihtml(html);
        assert!(mini.contains("<p>Hello</p>"));
        assert!(mini.contains("<p>World</p>"));
        assert!(!mini.contains("Enable JS"));
    }

    #[test]
    fn test_inline_formatting() {
        let html = "<p><b>Bold</b> and <i>italic</i></p>";
        let mini = html_to_minihtml(html);
        assert_eq!(mini, "<p><b>Bold</b> and <i>italic</i></p>");
    }
}
