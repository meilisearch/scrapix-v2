use anyhow::Result;
use colored::Colorize;

use crate::client::ApiClient;
use crate::output::print_json;
use crate::types::{SearchRequest, SearchResponse};

#[allow(clippy::too_many_arguments)]
pub async fn handle_search(
    client: &ApiClient,
    url: String,
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    filter: Option<String>,
    sort: Vec<String>,
    json: bool,
) -> Result<()> {
    let request = SearchRequest {
        url,
        q: query.clone(),
        limit,
        offset,
        filter,
        sort: if sort.is_empty() { None } else { Some(sort) },
    };

    let response: SearchResponse = client.post("/search", &request).await?;

    if json {
        print_json(&response);
    } else {
        if response.hits.is_empty() {
            eprintln!("No results for \"{}\"", query);
            return Ok(());
        }

        for (i, hit) in response.hits.iter().enumerate() {
            let title = hit
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            let url = hit.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let snippet = hit
                .get("_formatted")
                .and_then(|f| f.get("content"))
                .and_then(|v| v.as_str())
                .or_else(|| hit.get("description").and_then(|v| v.as_str()))
                .unwrap_or("");

            println!("{}. {}", (i + 1).to_string().bold(), title.bold());
            if !url.is_empty() {
                println!("   {}", url.cyan());
            }
            if !snippet.is_empty() {
                let truncated = if snippet.len() > 200 {
                    format!("{}...", &snippet[..200])
                } else {
                    snippet.to_string()
                };
                println!("   {}", truncated.dimmed());
            }
            println!();
        }

        if let Some(total) = response.estimated_total_hits {
            eprintln!("{} {} results for \"{}\"", "ℹ".blue().bold(), total, query);
        }
        if let Some(time) = response.processing_time_ms {
            eprintln!("{}", format!("Search took {}ms", time).dimmed());
        }
    }

    Ok(())
}
