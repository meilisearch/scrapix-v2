//! Custom CSS selector extraction from HTML documents
//!
//! Allows extracting specific data using CSS selectors with configurable
//! extraction modes (text, html, attribute, list).

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use tracing::{debug, instrument, warn};

/// Errors that can occur during selector extraction
#[derive(Debug, Error)]
pub enum SelectorError {
    #[error("Invalid CSS selector '{selector}': {message}")]
    InvalidSelector { selector: String, message: String },

    #[error("Extraction error: {0}")]
    ExtractionError(String),
}

/// How to extract the value from matched elements
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtractionMode {
    /// Extract text content (default)
    #[default]
    Text,
    /// Extract inner HTML
    Html,
    /// Extract outer HTML (including the element itself)
    OuterHtml,
    /// Extract a specific attribute value
    Attribute(String),
    /// Extract as a list of values (for multiple matches)
    List,
    /// Extract as list of objects with multiple fields
    ListOfObjects,
    /// Extract first match only (returns null if not found)
    First,
    /// Count matching elements
    Count,
}

/// Definition for a single selector extraction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectorDefinition {
    /// CSS selector(s) to match
    #[serde(flatten)]
    pub selector: SelectorInput,

    /// How to extract the value
    #[serde(default)]
    pub mode: ExtractionMode,

    /// Attribute name (for Attribute mode)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,

    /// Default value if no match found
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,

    /// Transform the extracted value (trim, lowercase, etc.)
    #[serde(default)]
    pub transform: Vec<Transform>,

    /// For ListOfObjects mode: field definitions
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub fields: HashMap<String, FieldDefinition>,
}

/// Input selector format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SelectorInput {
    /// Single CSS selector
    Single { selector: String },
    /// Multiple CSS selectors (first match wins)
    Multiple { selectors: Vec<String> },
}

impl SelectorInput {
    pub fn selectors(&self) -> Vec<&str> {
        match self {
            SelectorInput::Single { selector } => vec![selector.as_str()],
            SelectorInput::Multiple { selectors } => selectors.iter().map(|s| s.as_str()).collect(),
        }
    }
}

/// Field definition for ListOfObjects mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    /// CSS selector relative to parent element
    pub selector: String,

    /// How to extract this field
    #[serde(default)]
    pub mode: ExtractionMode,

    /// Attribute name if mode is Attribute
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attribute: Option<String>,
}

/// Value transformations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Transform {
    /// Trim whitespace
    Trim,
    /// Convert to lowercase
    Lowercase,
    /// Convert to uppercase
    Uppercase,
    /// Remove extra whitespace
    NormalizeWhitespace,
    /// Parse as number (returns null if fails)
    ParseNumber,
    /// Parse as boolean
    ParseBoolean,
    /// Remove HTML tags
    StripTags,
    /// Take first N characters
    Truncate(usize),
    /// Split by delimiter
    Split(String),
    /// Join array with delimiter
    Join(String),
    /// Replace substring
    Replace { from: String, to: String },
    /// Extract with regex
    Regex(String),
}

/// Result of selector extraction
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedSelectors {
    /// Extracted values by field name
    #[serde(flatten)]
    pub values: HashMap<String, Value>,

    /// Number of extractions performed
    pub extraction_count: usize,

    /// Fields that had no matches
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_fields: Vec<String>,
}

/// Custom selector extractor
pub struct SelectorExtractor {
    definitions: HashMap<String, SelectorDefinition>,
}

impl Default for SelectorExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl SelectorExtractor {
    /// Create a new empty extractor
    pub fn new() -> Self {
        Self {
            definitions: HashMap::new(),
        }
    }

    /// Create extractor from selector definitions
    pub fn with_definitions(definitions: HashMap<String, SelectorDefinition>) -> Self {
        Self { definitions }
    }

    /// Create extractor from simple selector map (selector string -> field name)
    pub fn from_simple(selectors: HashMap<String, String>) -> Self {
        let definitions = selectors
            .into_iter()
            .map(|(field, selector)| {
                (
                    field,
                    SelectorDefinition {
                        selector: SelectorInput::Single { selector },
                        mode: ExtractionMode::Text,
                        attribute: None,
                        default: None,
                        transform: vec![Transform::Trim],
                        fields: HashMap::new(),
                    },
                )
            })
            .collect();
        Self { definitions }
    }

    /// Add a selector definition
    pub fn add(&mut self, name: impl Into<String>, definition: SelectorDefinition) {
        self.definitions.insert(name.into(), definition);
    }

    /// Add a simple text selector
    pub fn add_text(&mut self, name: impl Into<String>, selector: impl Into<String>) {
        self.add(
            name,
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: selector.into(),
                },
                mode: ExtractionMode::Text,
                attribute: None,
                default: None,
                transform: vec![Transform::Trim],
                fields: HashMap::new(),
            },
        );
    }

    /// Add an attribute selector
    pub fn add_attribute(
        &mut self,
        name: impl Into<String>,
        selector: impl Into<String>,
        attribute: impl Into<String>,
    ) {
        self.add(
            name,
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: selector.into(),
                },
                mode: ExtractionMode::Attribute(attribute.into()),
                attribute: None,
                default: None,
                transform: vec![Transform::Trim],
                fields: HashMap::new(),
            },
        );
    }

    /// Add a list selector
    pub fn add_list(&mut self, name: impl Into<String>, selector: impl Into<String>) {
        self.add(
            name,
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: selector.into(),
                },
                mode: ExtractionMode::List,
                attribute: None,
                default: None,
                transform: vec![Transform::Trim],
                fields: HashMap::new(),
            },
        );
    }

    /// Extract values from HTML
    #[instrument(skip(self, html), level = "debug")]
    pub fn extract(&self, html: &str) -> Result<ExtractedSelectors, SelectorError> {
        let document = Html::parse_document(html);
        let mut result = ExtractedSelectors::default();

        for (field_name, definition) in &self.definitions {
            match self.extract_field(&document, definition) {
                Ok(value) => {
                    if value != Value::Null || definition.default.is_some() {
                        let final_value = if value == Value::Null {
                            definition.default.clone().unwrap_or(Value::Null)
                        } else {
                            value
                        };
                        result.values.insert(field_name.clone(), final_value);
                        result.extraction_count += 1;
                    } else {
                        result.missing_fields.push(field_name.clone());
                    }
                }
                Err(e) => {
                    warn!(field = %field_name, error = %e, "Failed to extract field");
                    if let Some(default) = &definition.default {
                        result.values.insert(field_name.clone(), default.clone());
                    } else {
                        result.missing_fields.push(field_name.clone());
                    }
                }
            }
        }

        debug!(
            extraction_count = result.extraction_count,
            missing_count = result.missing_fields.len(),
            "Extracted custom selectors"
        );

        Ok(result)
    }

    /// Extract a single field
    fn extract_field(
        &self,
        document: &Html,
        definition: &SelectorDefinition,
    ) -> Result<Value, SelectorError> {
        // Try each selector until one matches
        for selector_str in definition.selector.selectors() {
            let selector = Selector::parse(selector_str).map_err(|e| SelectorError::InvalidSelector {
                selector: selector_str.to_string(),
                message: format!("{:?}", e),
            })?;

            let elements: Vec<_> = document.select(&selector).collect();

            if elements.is_empty() {
                continue;
            }

            // Extract based on mode
            let value = match &definition.mode {
                ExtractionMode::Text => {
                    let text = elements[0].text().collect::<String>();
                    Value::String(text)
                }
                ExtractionMode::Html => {
                    let html = elements[0].inner_html();
                    Value::String(html)
                }
                ExtractionMode::OuterHtml => {
                    let html = elements[0].html();
                    Value::String(html)
                }
                ExtractionMode::Attribute(attr) => {
                    if let Some(value) = elements[0].value().attr(attr) {
                        Value::String(value.to_string())
                    } else {
                        Value::Null
                    }
                }
                ExtractionMode::First => {
                    let text = elements[0].text().collect::<String>();
                    Value::String(text)
                }
                ExtractionMode::List => {
                    let values: Vec<Value> = elements
                        .iter()
                        .map(|el| Value::String(el.text().collect::<String>()))
                        .collect();
                    Value::Array(values)
                }
                ExtractionMode::ListOfObjects => {
                    let objects: Vec<Value> = elements
                        .iter()
                        .map(|el| self.extract_object(el, &definition.fields))
                        .collect();
                    Value::Array(objects)
                }
                ExtractionMode::Count => Value::Number(elements.len().into()),
            };

            // Apply transforms
            let transformed = self.apply_transforms(value, &definition.transform);
            return Ok(transformed);
        }

        Ok(Value::Null)
    }

    /// Extract an object from an element using field definitions
    fn extract_object(
        &self,
        element: &scraper::ElementRef,
        fields: &HashMap<String, FieldDefinition>,
    ) -> Value {
        let mut obj = serde_json::Map::new();

        for (field_name, field_def) in fields {
            if let Ok(selector) = Selector::parse(&field_def.selector) {
                if let Some(el) = element.select(&selector).next() {
                    let value = match &field_def.mode {
                        ExtractionMode::Text | ExtractionMode::First => {
                            Value::String(el.text().collect::<String>().trim().to_string())
                        }
                        ExtractionMode::Html => Value::String(el.inner_html()),
                        ExtractionMode::OuterHtml => Value::String(el.html()),
                        ExtractionMode::Attribute(attr) => {
                            if let Some(v) = el.value().attr(attr) {
                                Value::String(v.to_string())
                            } else {
                                Value::Null
                            }
                        }
                        _ => Value::String(el.text().collect::<String>().trim().to_string()),
                    };
                    obj.insert(field_name.clone(), value);
                }
            }
        }

        Value::Object(obj)
    }

    /// Apply transforms to a value
    fn apply_transforms(&self, mut value: Value, transforms: &[Transform]) -> Value {
        for transform in transforms {
            value = match transform {
                Transform::Trim => {
                    if let Value::String(s) = value {
                        Value::String(s.trim().to_string())
                    } else if let Value::Array(arr) = value {
                        Value::Array(
                            arr.into_iter()
                                .map(|v| {
                                    if let Value::String(s) = v {
                                        Value::String(s.trim().to_string())
                                    } else {
                                        v
                                    }
                                })
                                .collect(),
                        )
                    } else {
                        value
                    }
                }
                Transform::Lowercase => {
                    if let Value::String(s) = value {
                        Value::String(s.to_lowercase())
                    } else {
                        value
                    }
                }
                Transform::Uppercase => {
                    if let Value::String(s) = value {
                        Value::String(s.to_uppercase())
                    } else {
                        value
                    }
                }
                Transform::NormalizeWhitespace => {
                    if let Value::String(s) = value {
                        let normalized: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
                        Value::String(normalized)
                    } else {
                        value
                    }
                }
                Transform::ParseNumber => {
                    if let Value::String(s) = &value {
                        // Remove non-numeric chars except . and -
                        let cleaned: String = s
                            .chars()
                            .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                            .collect();
                        if let Ok(n) = cleaned.parse::<f64>() {
                            if n.fract() == 0.0 {
                                Value::Number((n as i64).into())
                            } else {
                                serde_json::Number::from_f64(n)
                                    .map(Value::Number)
                                    .unwrap_or(value)
                            }
                        } else {
                            Value::Null
                        }
                    } else {
                        value
                    }
                }
                Transform::ParseBoolean => {
                    if let Value::String(s) = &value {
                        let lower = s.to_lowercase();
                        Value::Bool(
                            lower == "true"
                                || lower == "yes"
                                || lower == "1"
                                || lower == "on",
                        )
                    } else {
                        value
                    }
                }
                Transform::StripTags => {
                    if let Value::String(s) = value {
                        // Simple tag stripping
                        let stripped = Html::parse_fragment(&s)
                            .root_element()
                            .text()
                            .collect::<String>();
                        Value::String(stripped)
                    } else {
                        value
                    }
                }
                Transform::Truncate(len) => {
                    if let Value::String(s) = value {
                        let truncated: String = s.chars().take(*len).collect();
                        Value::String(truncated)
                    } else {
                        value
                    }
                }
                Transform::Split(delimiter) => {
                    if let Value::String(s) = value {
                        let parts: Vec<Value> = s
                            .split(delimiter)
                            .map(|p| Value::String(p.trim().to_string()))
                            .collect();
                        Value::Array(parts)
                    } else {
                        value
                    }
                }
                Transform::Join(delimiter) => {
                    if let Value::Array(arr) = value {
                        let strings: Vec<String> = arr
                            .into_iter()
                            .filter_map(|v| {
                                if let Value::String(s) = v {
                                    Some(s)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        Value::String(strings.join(delimiter))
                    } else {
                        value
                    }
                }
                Transform::Replace { from, to } => {
                    if let Value::String(s) = value {
                        Value::String(s.replace(from, to))
                    } else {
                        value
                    }
                }
                Transform::Regex(pattern) => {
                    if let Value::String(s) = &value {
                        if let Ok(re) = regex::Regex::new(pattern) {
                            if let Some(caps) = re.captures(s) {
                                if let Some(m) = caps.get(1) {
                                    return Value::String(m.as_str().to_string());
                                } else if let Some(m) = caps.get(0) {
                                    return Value::String(m.as_str().to_string());
                                }
                            }
                        }
                        Value::Null
                    } else {
                        value
                    }
                }
            };
        }
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h1 class="title">Hello World</h1>
                <p class="content">This is content</p>
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add_text("title", "h1.title");
        extractor.add_text("content", "p.content");

        let result = extractor.extract(html).unwrap();

        assert_eq!(result.values.get("title").unwrap(), "Hello World");
        assert_eq!(result.values.get("content").unwrap(), "This is content");
    }

    #[test]
    fn test_extract_attribute() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <a href="https://example.com" class="link">Link</a>
                <img src="/image.jpg" alt="Image">
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add_attribute("link_url", "a.link", "href");
        extractor.add_attribute("image_src", "img", "src");
        extractor.add_attribute("image_alt", "img", "alt");

        let result = extractor.extract(html).unwrap();

        assert_eq!(result.values.get("link_url").unwrap(), "https://example.com");
        assert_eq!(result.values.get("image_src").unwrap(), "/image.jpg");
        assert_eq!(result.values.get("image_alt").unwrap(), "Image");
    }

    #[test]
    fn test_extract_list() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <ul>
                    <li>Item 1</li>
                    <li>Item 2</li>
                    <li>Item 3</li>
                </ul>
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add_list("items", "li");

        let result = extractor.extract(html).unwrap();

        let items = result.values.get("items").unwrap().as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0], "Item 1");
        assert_eq!(items[1], "Item 2");
        assert_eq!(items[2], "Item 3");
    }

    #[test]
    fn test_extract_with_transforms() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <span class="price">$99.99</span>
                <span class="name">  PRODUCT NAME  </span>
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add(
            "price",
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: ".price".to_string(),
                },
                mode: ExtractionMode::Text,
                attribute: None,
                default: None,
                transform: vec![Transform::ParseNumber],
                fields: HashMap::new(),
            },
        );
        extractor.add(
            "name",
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: ".name".to_string(),
                },
                mode: ExtractionMode::Text,
                attribute: None,
                default: None,
                transform: vec![Transform::Trim, Transform::Lowercase],
                fields: HashMap::new(),
            },
        );

        let result = extractor.extract(html).unwrap();

        assert_eq!(result.values.get("price").unwrap(), 99.99);
        assert_eq!(result.values.get("name").unwrap(), "product name");
    }

    #[test]
    fn test_extract_count() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <div class="item">1</div>
                <div class="item">2</div>
                <div class="item">3</div>
                <div class="item">4</div>
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add(
            "item_count",
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: ".item".to_string(),
                },
                mode: ExtractionMode::Count,
                attribute: None,
                default: None,
                transform: vec![],
                fields: HashMap::new(),
            },
        );

        let result = extractor.extract(html).unwrap();

        assert_eq!(result.values.get("item_count").unwrap(), 4);
    }

    #[test]
    fn test_extract_with_default() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h1>Title</h1>
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add(
            "subtitle",
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: "h2.subtitle".to_string(),
                },
                mode: ExtractionMode::Text,
                attribute: None,
                default: Some(Value::String("No subtitle".to_string())),
                transform: vec![],
                fields: HashMap::new(),
            },
        );

        let result = extractor.extract(html).unwrap();

        assert_eq!(result.values.get("subtitle").unwrap(), "No subtitle");
    }

    #[test]
    fn test_extract_html() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <div class="content"><strong>Bold</strong> text</div>
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add(
            "content_html",
            SelectorDefinition {
                selector: SelectorInput::Single {
                    selector: ".content".to_string(),
                },
                mode: ExtractionMode::Html,
                attribute: None,
                default: None,
                transform: vec![],
                fields: HashMap::new(),
            },
        );

        let result = extractor.extract(html).unwrap();

        assert!(result
            .values
            .get("content_html")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("<strong>Bold</strong>"));
    }

    #[test]
    fn test_multiple_selectors_fallback() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <span class="alt-title">Alt Title</span>
            </body>
            </html>
        "#;

        let mut extractor = SelectorExtractor::new();
        extractor.add(
            "title",
            SelectorDefinition {
                selector: SelectorInput::Multiple {
                    selectors: vec!["h1.title".to_string(), ".alt-title".to_string()],
                },
                mode: ExtractionMode::Text,
                attribute: None,
                default: None,
                transform: vec![Transform::Trim],
                fields: HashMap::new(),
            },
        );

        let result = extractor.extract(html).unwrap();

        assert_eq!(result.values.get("title").unwrap(), "Alt Title");
    }

    #[test]
    fn test_from_simple() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <h1>Title</h1>
                <p>Content</p>
            </body>
            </html>
        "#;

        let selectors: HashMap<String, String> = [
            ("title".to_string(), "h1".to_string()),
            ("content".to_string(), "p".to_string()),
        ]
        .into_iter()
        .collect();

        let extractor = SelectorExtractor::from_simple(selectors);
        let result = extractor.extract(html).unwrap();

        assert_eq!(result.values.get("title").unwrap(), "Title");
        assert_eq!(result.values.get("content").unwrap(), "Content");
    }
}
