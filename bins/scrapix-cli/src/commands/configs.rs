use anyhow::{Context, Result};
use colored::Colorize;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{print_info, print_json, print_success};
use crate::types::{ConfigRow, CrawlConfigRecord, CreateCrawlResponse};

pub async fn handle_configs_list(
    client: &ApiClient,
    limit: usize,
    offset: usize,
    json: bool,
) -> Result<()> {
    let configs: Vec<CrawlConfigRecord> = client
        .get(&format!("/configs?limit={}&offset={}", limit, offset))
        .await?;

    if json {
        print_json(&configs);
    } else {
        if configs.is_empty() {
            print_info("No configs found");
            return Ok(());
        }

        let rows: Vec<ConfigRow> = configs
            .iter()
            .map(|c| ConfigRow {
                id: if c.id.len() > 8 {
                    format!("{}...", &c.id[..8])
                } else {
                    c.id.clone()
                },
                name: c.name.clone(),
                cron: c.cron_expression.clone().unwrap_or_else(|| "-".to_string()),
                enabled: if c.cron_enabled.unwrap_or(false) {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
                last_run: c.last_run_at.clone().unwrap_or_else(|| "-".to_string()),
            })
            .collect();

        println!("{}", Table::new(rows));
    }

    Ok(())
}

pub async fn handle_config_show(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    let config: CrawlConfigRecord = client.get(&format!("/configs/{}", id)).await?;

    if json {
        print_json(&config);
    } else {
        eprintln!();
        eprintln!("{}", "Config Details".bold().underline());
        eprintln!();
        eprintln!("  {} {}", "ID:".dimmed(), config.id);
        eprintln!("  {} {}", "Name:".dimmed(), config.name);
        if let Some(ref desc) = config.description {
            eprintln!("  {} {}", "Description:".dimmed(), desc);
        }
        if let Some(ref cron) = config.cron_expression {
            eprintln!("  {} {}", "Cron:".dimmed(), cron);
        }
        eprintln!(
            "  {} {}",
            "Enabled:".dimmed(),
            if config.cron_enabled.unwrap_or(false) {
                "yes".green()
            } else {
                "no".red()
            }
        );
        if let Some(ref last_run) = config.last_run_at {
            eprintln!("  {} {}", "Last Run:".dimmed(), last_run);
        }
        if let Some(ref last_job) = config.last_job_id {
            eprintln!("  {} {}", "Last Job:".dimmed(), last_job);
        }
        eprintln!();
        eprintln!("{}", "Config JSON:".bold());
        eprintln!("{}", serde_json::to_string_pretty(&config.config).unwrap());
        eprintln!();
    }

    Ok(())
}

pub async fn handle_config_create(client: &ApiClient, file: &str, json: bool) -> Result<()> {
    let content =
        std::fs::read_to_string(file).with_context(|| format!("Failed to read file: {}", file))?;
    let body: serde_json::Value =
        serde_json::from_str(&content).with_context(|| "Failed to parse JSON")?;

    let config: CrawlConfigRecord = client.post("/configs", &body).await?;

    if json {
        print_json(&config);
    } else {
        print_success(&format!("Config created: {}", config.id.cyan()));
    }

    Ok(())
}

pub async fn handle_config_update(
    client: &ApiClient,
    id: &str,
    file: &str,
    json: bool,
) -> Result<()> {
    let content =
        std::fs::read_to_string(file).with_context(|| format!("Failed to read file: {}", file))?;
    let body: serde_json::Value =
        serde_json::from_str(&content).with_context(|| "Failed to parse JSON")?;

    let config: CrawlConfigRecord = client.patch(&format!("/configs/{}", id), &body).await?;

    if json {
        print_json(&config);
    } else {
        print_success(&format!("Config {} updated", id.cyan()));
    }

    Ok(())
}

pub async fn handle_config_delete(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    client.delete_no_body(&format!("/configs/{}", id)).await?;

    if json {
        print_json(&serde_json::json!({"deleted": id}));
    } else {
        print_success(&format!("Config {} deleted", id.cyan()));
    }

    Ok(())
}

pub async fn handle_config_trigger(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    let response: CreateCrawlResponse = client
        .post(&format!("/configs/{}/trigger", id), &serde_json::json!({}))
        .await?;

    if json {
        print_json(&response);
    } else {
        print_success(&format!(
            "Config {} triggered — job {}",
            id.cyan(),
            response.job_id.cyan()
        ));
    }

    Ok(())
}
