use anyhow::Result;
use colored::Colorize;

use crate::client::ApiClient;
use crate::output::{create_spinner, print_json};
use crate::types::{ScrapeAiOptions, ScrapeRequest, ScrapeResponse};

#[allow(clippy::too_many_arguments)]
pub async fn handle_scrape(
    client: &ApiClient,
    url: String,
    format: Option<String>,
    main_content: bool,
    js: bool,
    timeout: Option<u64>,
    extract: Vec<String>,
    ai_summary: bool,
    ai_extract: Option<String>,
    json: bool,
) -> Result<()> {
    let _ = main_content; // reserved for future use
    let fmt = format.unwrap_or_else(|| "markdown".to_string());

    // Build selector from extract flags
    let selector = if extract.is_empty() {
        None
    } else {
        // Join multiple selectors
        Some(extract.join(", "))
    };

    let ai_options = if ai_summary || ai_extract.is_some() {
        Some(ScrapeAiOptions {
            summary: if ai_summary { Some(true) } else { None },
            extraction_prompt: ai_extract,
        })
    } else {
        None
    };

    let request = ScrapeRequest {
        url: url.clone(),
        format: Some(fmt.clone()),
        selector,
        js_render: if js { Some(true) } else { None },
        timeout,
        ai_options,
    };

    let spinner = if !json {
        Some(create_spinner(&format!("Scraping {}", url)))
    } else {
        None
    };

    let response: ScrapeResponse = client.post("/scrape", &request).await?;

    if let Some(sp) = spinner {
        sp.finish_and_clear();
    }

    if json {
        print_json(&response);
    } else {
        // Output the content to stdout for piping
        match fmt.as_str() {
            "md" | "markdown" => {
                if let Some(ref md) = response.markdown {
                    println!("{}", md);
                } else if let Some(ref content) = response.content {
                    println!("{}", content);
                }
            }
            "html" => {
                if let Some(ref html) = response.html {
                    println!("{}", html);
                }
            }
            "raw" | "rawhtml" | "raw_html" => {
                if let Some(ref raw) = response.raw_html {
                    println!("{}", raw);
                }
            }
            "text" | "content" => {
                if let Some(ref content) = response.content {
                    println!("{}", content);
                }
            }
            "links" => {
                if let Some(ref links) = response.links {
                    for link in links {
                        println!("{}", link);
                    }
                }
            }
            "metadata" => {
                if let Some(ref metadata) = response.metadata {
                    println!("{}", serde_json::to_string_pretty(metadata)?);
                }
            }
            _ => {
                if let Some(ref md) = response.markdown {
                    println!("{}", md);
                }
            }
        }

        // Show AI results on stderr
        if let Some(ref summary) = response.ai_summary {
            eprintln!();
            eprintln!("{}", "AI Summary".bold().underline());
            eprintln!("{}", summary);
        }
        if let Some(ref extraction) = response.ai_extraction {
            eprintln!();
            eprintln!("{}", "AI Extraction".bold().underline());
            eprintln!("{}", serde_json::to_string_pretty(extraction)?);
        }
    }

    Ok(())
}
