use anyhow::Result;
use colored::Colorize;

use crate::client::ApiClient;
use crate::output::{create_spinner, print_json};
use crate::types::{MapRequest, MapResponse};

#[allow(clippy::too_many_arguments)]
pub async fn handle_map(
    client: &ApiClient,
    url: String,
    limit: Option<u32>,
    depth: Option<u32>,
    search: Option<String>,
    no_sitemap: bool,
    no_metadata: bool,
    json: bool,
) -> Result<()> {
    let request = MapRequest {
        url: url.clone(),
        limit,
        depth,
        search,
        no_sitemap: if no_sitemap { Some(true) } else { None },
        no_metadata: if no_metadata { Some(true) } else { None },
    };

    let spinner = if !json {
        Some(create_spinner(&format!("Mapping {}", url)))
    } else {
        None
    };

    let response: MapResponse = client.post("/map", &request).await?;

    if let Some(sp) = spinner {
        sp.finish_and_clear();
    }

    if json {
        print_json(&response);
    } else {
        // Output URLs to stdout (one per line, piping-friendly)
        for link in &response.links {
            if no_metadata {
                println!("{}", link.url);
            } else if let Some(ref title) = link.title {
                println!("{} {}", link.url, format!("- {}", title).dimmed());
            } else {
                println!("{}", link.url);
            }
        }

        eprintln!();
        eprintln!(
            "{} {} URLs found on {}",
            "ℹ".blue().bold(),
            response.total,
            url
        );
    }

    Ok(())
}
