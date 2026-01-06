//! Schema.org and JSON-LD extraction from HTML documents
//!
//! Extracts structured data in JSON-LD, Microdata, and RDFa formats.

use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use thiserror::Error;
use tracing::{debug, instrument, warn};

/// Errors that can occur during schema extraction
#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("Invalid JSON-LD: {0}")]
    InvalidJsonLd(String),

    #[error("Invalid selector: {0}")]
    InvalidSelector(String),

    #[error("Extraction error: {0}")]
    ExtractionError(String),
}

/// Configuration for schema extraction
#[derive(Debug, Clone, Default)]
pub struct SchemaConfig {
    /// Only extract these schema types (empty = all types)
    pub only_types: HashSet<String>,

    /// Convert ISO date strings to timestamps
    pub convert_dates: bool,

    /// Flatten nested @graph structures
    pub flatten_graph: bool,

    /// Include raw JSON-LD scripts in output
    pub include_raw: bool,
}

/// Extracted schema data from an HTML document
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExtractedSchema {
    /// All JSON-LD data found (may be array or object)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_ld: Option<Value>,

    /// Individual schema items by type
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<SchemaItem>,

    /// Microdata items
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub microdata: Vec<SchemaItem>,

    /// Number of JSON-LD scripts found
    pub json_ld_count: usize,

    /// Number of microdata items found
    pub microdata_count: usize,
}

/// A single schema.org item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaItem {
    /// Schema type (e.g., "Article", "Product", "Organization")
    #[serde(rename = "@type")]
    pub schema_type: String,

    /// Full schema data
    #[serde(flatten)]
    pub data: Value,
}

/// Schema extractor
pub struct SchemaExtractor {
    config: SchemaConfig,
    jsonld_selector: Selector,
    microdata_selector: Selector,
}

impl Default for SchemaExtractor {
    fn default() -> Self {
        Self::new(SchemaConfig::default())
    }
}

impl SchemaExtractor {
    /// Create a new schema extractor with configuration
    pub fn new(config: SchemaConfig) -> Self {
        Self {
            config,
            jsonld_selector: Selector::parse("script[type='application/ld+json']")
                .expect("valid jsonld selector"),
            microdata_selector: Selector::parse("[itemscope]").expect("valid microdata selector"),
        }
    }

    /// Create extractor with default config
    pub fn with_defaults() -> Self {
        Self::default()
    }

    /// Create extractor that only extracts specific types
    pub fn with_types(types: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::new(SchemaConfig {
            only_types: types.into_iter().map(|t| t.into()).collect(),
            ..Default::default()
        })
    }

    /// Extract all schema data from an HTML document
    #[instrument(skip(self, html), level = "debug")]
    pub fn extract(&self, html: &str) -> Result<ExtractedSchema, SchemaError> {
        let document = Html::parse_document(html);
        let mut result = ExtractedSchema::default();

        // Extract JSON-LD
        self.extract_json_ld(&document, &mut result)?;

        // Extract Microdata
        self.extract_microdata(&document, &mut result);

        debug!(
            json_ld_count = result.json_ld_count,
            microdata_count = result.microdata_count,
            items_count = result.items.len(),
            "Extracted schema data"
        );

        Ok(result)
    }

    /// Extract JSON-LD scripts
    fn extract_json_ld(
        &self,
        document: &Html,
        result: &mut ExtractedSchema,
    ) -> Result<(), SchemaError> {
        let mut all_json_ld: Vec<Value> = Vec::new();

        for script in document.select(&self.jsonld_selector) {
            let content = script.text().collect::<String>();
            let content = content.trim();

            if content.is_empty() {
                continue;
            }

            // Parse JSON
            match serde_json::from_str::<Value>(content) {
                Ok(json) => {
                    result.json_ld_count += 1;

                    // Process the JSON-LD
                    self.process_json_ld(&json, result);

                    // Store raw JSON
                    all_json_ld.push(json);
                }
                Err(e) => {
                    warn!("Failed to parse JSON-LD: {}", e);
                    // Continue processing other scripts
                }
            }
        }

        // Combine all JSON-LD into result
        if !all_json_ld.is_empty() {
            result.json_ld = if all_json_ld.len() == 1 {
                Some(all_json_ld.into_iter().next().unwrap())
            } else {
                Some(Value::Array(all_json_ld))
            };
        }

        Ok(())
    }

    /// Process a JSON-LD value and extract schema items
    fn process_json_ld(&self, json: &Value, result: &mut ExtractedSchema) {
        match json {
            Value::Array(arr) => {
                for item in arr {
                    self.process_json_ld(item, result);
                }
            }
            Value::Object(obj) => {
                // Check for @graph (multiple items in one script)
                if let Some(graph) = obj.get("@graph") {
                    self.process_json_ld(graph, result);
                    return;
                }

                // Extract @type
                if let Some(type_value) = obj.get("@type") {
                    let types = self.extract_types(type_value);

                    for schema_type in types {
                        // Filter by configured types if any
                        if !self.config.only_types.is_empty()
                            && !self.config.only_types.contains(&schema_type)
                        {
                            continue;
                        }

                        let mut data = json.clone();

                        // Convert dates if configured
                        if self.config.convert_dates {
                            self.convert_dates_in_value(&mut data);
                        }

                        result.items.push(SchemaItem { schema_type, data });
                    }
                }
            }
            _ => {}
        }
    }

    /// Extract type(s) from @type value (can be string or array)
    fn extract_types(&self, type_value: &Value) -> Vec<String> {
        match type_value {
            Value::String(s) => vec![self.normalize_type(s)],
            Value::Array(arr) => arr
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| self.normalize_type(s))
                .collect(),
            _ => vec![],
        }
    }

    /// Normalize schema type (remove schema.org prefix)
    fn normalize_type(&self, type_str: &str) -> String {
        type_str
            .trim()
            .replace("https://schema.org/", "")
            .replace("http://schema.org/", "")
    }

    /// Convert ISO date strings to timestamps in a JSON value
    fn convert_dates_in_value(&self, value: &mut Value) {
        match value {
            Value::Object(obj) => {
                for (key, val) in obj.iter_mut() {
                    // Check if this looks like a date field
                    let is_date_field = key.to_lowercase().contains("date")
                        || key.to_lowercase().contains("time")
                        || key == "datePublished"
                        || key == "dateModified"
                        || key == "dateCreated"
                        || key == "startDate"
                        || key == "endDate";

                    if is_date_field {
                        if let Value::String(s) = val {
                            // Try to parse ISO date and convert to timestamp
                            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                                *val = Value::Number(dt.timestamp().into());
                            } else if let Ok(dt) =
                                chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                            {
                                let datetime = dt
                                    .and_hms_opt(0, 0, 0)
                                    .unwrap()
                                    .and_utc();
                                *val = Value::Number(datetime.timestamp().into());
                            }
                        }
                    } else {
                        self.convert_dates_in_value(val);
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr.iter_mut() {
                    self.convert_dates_in_value(item);
                }
            }
            _ => {}
        }
    }

    /// Extract Microdata items
    fn extract_microdata(&self, document: &Html, result: &mut ExtractedSchema) {
        // Only process top-level itemscope elements (not nested)
        for element in document.select(&self.microdata_selector) {
            // Skip if this is nested inside another itemscope
            let mut parent = element.parent();
            let mut is_nested = false;
            while let Some(p) = parent {
                if p.value().is_element() {
                    if let Some(elem) = p.value().as_element() {
                        if elem.attr("itemscope").is_some() {
                            is_nested = true;
                            break;
                        }
                    }
                }
                parent = p.parent();
            }

            if is_nested {
                continue;
            }

            // Get itemtype
            if let Some(itemtype) = element.value().attr("itemtype") {
                let schema_type = self.normalize_type(itemtype);

                // Filter by configured types
                if !self.config.only_types.is_empty()
                    && !self.config.only_types.contains(&schema_type)
                {
                    continue;
                }

                // Extract properties
                let data = self.extract_microdata_properties(&element);

                result.microdata.push(SchemaItem {
                    schema_type: schema_type.clone(),
                    data,
                });
                result.microdata_count += 1;
            }
        }
    }

    /// Extract properties from a microdata element
    fn extract_microdata_properties(&self, element: &scraper::ElementRef) -> Value {
        let mut props: serde_json::Map<String, Value> = serde_json::Map::new();

        // Add @type
        if let Some(itemtype) = element.value().attr("itemtype") {
            props.insert("@type".to_string(), Value::String(self.normalize_type(itemtype)));
        }

        // Find all itemprop elements within this itemscope
        let itemprop_selector = Selector::parse("[itemprop]").expect("valid itemprop selector");

        for prop_elem in element.select(&itemprop_selector) {
            // Check if this itemprop is directly under our itemscope (not nested in another)
            let mut is_direct = true;
            let mut parent = prop_elem.parent();
            while let Some(p) = parent {
                if p.id() == element.id() {
                    break;
                }
                if p.value().is_element() {
                    if let Some(elem) = p.value().as_element() {
                        if elem.attr("itemscope").is_some() {
                            is_direct = false;
                            break;
                        }
                    }
                }
                parent = p.parent();
            }

            if !is_direct {
                continue;
            }

            if let Some(prop_name) = prop_elem.value().attr("itemprop") {
                let value = self.get_microdata_value(&prop_elem);

                // Handle multiple values for same property
                if let Some(existing) = props.get_mut(prop_name) {
                    match existing {
                        Value::Array(arr) => arr.push(value),
                        _ => {
                            let old = existing.clone();
                            *existing = Value::Array(vec![old, value]);
                        }
                    }
                } else {
                    props.insert(prop_name.to_string(), value);
                }
            }
        }

        Value::Object(props)
    }

    /// Get the value of a microdata property
    fn get_microdata_value(&self, element: &scraper::ElementRef) -> Value {
        // Check for nested itemscope
        if element.value().attr("itemscope").is_some() {
            return self.extract_microdata_properties(element);
        }

        // Get value based on element type
        let tag_name = element.value().name();

        let value = match tag_name {
            "meta" => element.value().attr("content"),
            "link" | "a" | "area" => element.value().attr("href"),
            "img" | "audio" | "video" | "source" => element.value().attr("src"),
            "object" => element.value().attr("data"),
            "time" => element.value().attr("datetime"),
            "data" | "meter" => element.value().attr("value"),
            _ => None,
        };

        if let Some(v) = value {
            Value::String(v.to_string())
        } else {
            // Fall back to text content
            let text = element.text().collect::<String>().trim().to_string();
            Value::String(text)
        }
    }

    /// Get a specific schema type from extracted data
    pub fn get_by_type<'a>(&self, result: &'a ExtractedSchema, schema_type: &str) -> Vec<&'a SchemaItem> {
        result
            .items
            .iter()
            .chain(result.microdata.iter())
            .filter(|item| item.schema_type == schema_type)
            .collect()
    }

    /// Extract a specific property from schema items of a given type
    pub fn get_property(&self, result: &ExtractedSchema, schema_type: &str, property: &str) -> Option<Value> {
        for item in self.get_by_type(result, schema_type) {
            if let Value::Object(obj) = &item.data {
                if let Some(value) = obj.get(property) {
                    return Some(value.clone());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_ld_article() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@type": "Article",
                    "headline": "Test Article",
                    "author": {
                        "@type": "Person",
                        "name": "John Doe"
                    },
                    "datePublished": "2024-01-15T10:00:00Z"
                }
                </script>
            </head>
            <body></body>
            </html>
        "#;

        let extractor = SchemaExtractor::default();
        let result = extractor.extract(html).unwrap();

        assert_eq!(result.json_ld_count, 1);
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].schema_type, "Article");

        if let Value::Object(obj) = &result.items[0].data {
            assert_eq!(obj.get("headline").unwrap(), "Test Article");
        }
    }

    #[test]
    fn test_extract_json_ld_graph() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@graph": [
                        {
                            "@type": "Organization",
                            "name": "Test Org"
                        },
                        {
                            "@type": "WebSite",
                            "name": "Test Site"
                        }
                    ]
                }
                </script>
            </head>
            <body></body>
            </html>
        "#;

        let extractor = SchemaExtractor::default();
        let result = extractor.extract(html).unwrap();

        assert_eq!(result.items.len(), 2);
        assert!(result.items.iter().any(|i| i.schema_type == "Organization"));
        assert!(result.items.iter().any(|i| i.schema_type == "WebSite"));
    }

    #[test]
    fn test_extract_json_ld_product() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@type": "Product",
                    "name": "Test Product",
                    "offers": {
                        "@type": "Offer",
                        "price": "99.99",
                        "priceCurrency": "USD"
                    }
                }
                </script>
            </head>
            <body></body>
            </html>
        "#;

        let extractor = SchemaExtractor::default();
        let result = extractor.extract(html).unwrap();

        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].schema_type, "Product");
    }

    #[test]
    fn test_filter_by_type() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@graph": [
                        { "@type": "Article", "headline": "Article" },
                        { "@type": "Organization", "name": "Org" },
                        { "@type": "Person", "name": "Person" }
                    ]
                }
                </script>
            </head>
            <body></body>
            </html>
        "#;

        let extractor = SchemaExtractor::with_types(["Article", "Person"]);
        let result = extractor.extract(html).unwrap();

        assert_eq!(result.items.len(), 2);
        assert!(!result.items.iter().any(|i| i.schema_type == "Organization"));
    }

    #[test]
    fn test_extract_microdata() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <body>
                <div itemscope itemtype="https://schema.org/Product">
                    <span itemprop="name">Test Product</span>
                    <span itemprop="description">A great product</span>
                    <meta itemprop="sku" content="12345">
                </div>
            </body>
            </html>
        "#;

        let extractor = SchemaExtractor::default();
        let result = extractor.extract(html).unwrap();

        assert_eq!(result.microdata_count, 1);
        assert_eq!(result.microdata.len(), 1);
        assert_eq!(result.microdata[0].schema_type, "Product");

        if let Value::Object(obj) = &result.microdata[0].data {
            assert_eq!(obj.get("name").unwrap(), "Test Product");
            assert_eq!(obj.get("sku").unwrap(), "12345");
        }
    }

    #[test]
    fn test_get_by_type() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@graph": [
                        { "@type": "Article", "headline": "Article 1" },
                        { "@type": "Article", "headline": "Article 2" }
                    ]
                }
                </script>
            </head>
            <body></body>
            </html>
        "#;

        let extractor = SchemaExtractor::default();
        let result = extractor.extract(html).unwrap();

        let articles = extractor.get_by_type(&result, "Article");
        assert_eq!(articles.len(), 2);
    }

    #[test]
    fn test_convert_dates() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <script type="application/ld+json">
                {
                    "@context": "https://schema.org",
                    "@type": "Article",
                    "datePublished": "2024-01-15T10:00:00Z"
                }
                </script>
            </head>
            <body></body>
            </html>
        "#;

        let extractor = SchemaExtractor::new(SchemaConfig {
            convert_dates: true,
            ..Default::default()
        });
        let result = extractor.extract(html).unwrap();

        if let Value::Object(obj) = &result.items[0].data {
            // Should be converted to timestamp
            assert!(obj.get("datePublished").unwrap().is_number());
        }
    }

    #[test]
    fn test_multiple_json_ld_scripts() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <script type="application/ld+json">
                { "@type": "Article", "headline": "First" }
                </script>
                <script type="application/ld+json">
                { "@type": "Organization", "name": "Second" }
                </script>
            </head>
            <body></body>
            </html>
        "#;

        let extractor = SchemaExtractor::default();
        let result = extractor.extract(html).unwrap();

        assert_eq!(result.json_ld_count, 2);
        assert_eq!(result.items.len(), 2);
    }
}
