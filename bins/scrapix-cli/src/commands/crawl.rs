use std::pin::pin;
use std::time::Duration;

use anyhow::{Context, Result};
use colored::Colorize;
use futures::StreamExt;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{create_spinner, print_error, print_info, print_json, print_success};
use crate::types::{CreateCrawlResponse, JobRow, JobStatusResponse};

use scrapix_core::CrawlConfig;

pub async fn handle_crawl(
    client: &ApiClient,
    config_path: Option<String>,
    inline_json: Option<String>,
    sync: bool,
    follow: bool,
    json: bool,
) -> Result<()> {
    let config = parse_config(config_path, inline_json)?;

    if config.start_urls.is_empty() {
        anyhow::bail!("Configuration must include at least one start_url");
    }
    if config.index_uid.is_empty() {
        anyhow::bail!("Configuration must include index_uid");
    }

    if !json {
        print_info(&format!(
            "Starting crawl job for {} URLs targeting index '{}'",
            config.start_urls.len(),
            config.index_uid
        ));
    }

    let spinner = if !json {
        Some(create_spinner(if sync {
            "Waiting for crawl to complete..."
        } else {
            "Submitting crawl job..."
        }))
    } else {
        None
    };

    let endpoint = if sync { "/crawl/sync" } else { "/crawl" };
    let response: CreateCrawlResponse = client.post(endpoint, &config).await?;

    if let Some(sp) = spinner {
        sp.finish_and_clear();
    }

    if json {
        print_json(&response);
    } else {
        print_success(&format!("Job created: {}", response.job_id.cyan()));
        eprintln!("  {} {}", "Status:".dimmed(), response.status);
        eprintln!("  {} {}", "Index:".dimmed(), response.index_uid);
        eprintln!("  {} {}", "URLs:".dimmed(), response.start_urls_count);
        eprintln!();

        if !sync && follow {
            print_info("Following job events (Ctrl+C to stop)...");
            eprintln!();
            handle_events(client, &response.job_id, json).await?;
        } else if !sync {
            eprintln!(
                "{}",
                format!("Run 'scrapix job {}' to check progress", response.job_id).dimmed()
            );
            eprintln!(
                "{}",
                format!(
                    "Run 'scrapix job {} --events' to stream events",
                    response.job_id
                )
                .dimmed()
            );
        }
    }

    Ok(())
}

pub async fn handle_jobs(
    client: &ApiClient,
    limit: usize,
    offset: usize,
    status_filter: Option<String>,
    json: bool,
) -> Result<()> {
    let mut path = format!("/jobs?limit={}&offset={}", limit, offset);
    if let Some(ref status) = status_filter {
        path.push_str(&format!("&status={}", status));
    }

    let jobs: Vec<JobStatusResponse> = client.get(&path).await?;

    if json {
        print_json(&jobs);
    } else {
        if jobs.is_empty() {
            print_info("No jobs found");
            return Ok(());
        }

        let rows: Vec<JobRow> = jobs.into_iter().map(JobRow::from).collect();
        let table = Table::new(rows).to_string();
        println!("{}", table);
    }

    Ok(())
}

pub async fn handle_job(
    client: &ApiClient,
    job_id: &str,
    watch: bool,
    events: bool,
    json: bool,
) -> Result<()> {
    if events {
        return handle_events(client, job_id, json).await;
    }

    if watch && !json {
        loop {
            print!("\x1B[2J\x1B[1;1H");
            let status: JobStatusResponse = client.get(&format!("/job/{}/status", job_id)).await?;
            print_job_status_text(&status);

            if matches!(status.status.as_str(), "completed" | "failed" | "cancelled") {
                break;
            }

            eprintln!("{}", "Refreshing every 2s... (Ctrl+C to stop)".dimmed());
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    } else {
        let status: JobStatusResponse = client.get(&format!("/job/{}/status", job_id)).await?;

        if json {
            print_json(&status);
        } else {
            print_job_status_text(&status);
        }
    }

    Ok(())
}

pub async fn handle_job_cancel(client: &ApiClient, job_id: &str, json: bool) -> Result<()> {
    let status: JobStatusResponse = client.delete(&format!("/job/{}", job_id)).await?;

    if json {
        print_json(&status);
    } else {
        print_success(&format!("Job {} cancelled", job_id.cyan()));
    }

    Ok(())
}

pub async fn handle_events(client: &ApiClient, job_id: &str, json: bool) -> Result<()> {
    let stream = client.stream_events(job_id).await?;
    let mut stream = pin!(stream);

    while let Some(result) = stream.next().await {
        match result {
            Ok(data) => {
                for line in data.lines() {
                    if let Some(json_str) = line.strip_prefix("data:") {
                        let json_str = json_str.trim();
                        if !json_str.is_empty() {
                            if json {
                                println!("{}", json_str);
                            } else if let Ok(event) =
                                serde_json::from_str::<serde_json::Value>(json_str)
                            {
                                print_event(&event);
                            } else {
                                println!("{}", json_str);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                print_error(&format!("Stream error: {}", e));
                break;
            }
        }
    }

    Ok(())
}

fn parse_config(config_path: Option<String>, inline_json: Option<String>) -> Result<CrawlConfig> {
    if let Some(path) = config_path {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path))
    } else if let Some(json) = inline_json {
        serde_json::from_str(&json).context("Failed to parse inline config")
    } else {
        anyhow::bail!("Config file path or --inline JSON required")
    }
}

fn print_job_status_text(job: &JobStatusResponse) {
    let status_color = match job.status.as_str() {
        "running" => "yellow",
        "completed" => "green",
        "failed" | "cancelled" => "red",
        _ => "white",
    };

    eprintln!();
    eprintln!("{}", "Job Status".bold().underline());
    eprintln!();
    eprintln!("  {} {}", "Job ID:".dimmed(), job.job_id);
    eprintln!(
        "  {} {}",
        "Status:".dimmed(),
        job.status.color(status_color).bold()
    );
    eprintln!("  {} {}", "Index:".dimmed(), job.index_uid);
    eprintln!();
    eprintln!("{}", "Progress".bold());
    eprintln!("  {} {}", "Pages Crawled:".dimmed(), job.pages_crawled);
    eprintln!("  {} {}", "Pages Indexed:".dimmed(), job.pages_indexed);
    eprintln!("  {} {}", "Documents Sent:".dimmed(), job.documents_sent);
    eprintln!(
        "  {} {}",
        "Errors:".dimmed(),
        if job.errors > 0 {
            job.errors.to_string().red().to_string()
        } else {
            job.errors.to_string()
        }
    );
    eprintln!("  {} {:.2}/s", "Crawl Rate:".dimmed(), job.crawl_rate);
    if let Some(eta) = job.eta_seconds {
        eprintln!("  {} {}s", "ETA:".dimmed(), eta);
    }

    eprintln!();
    eprintln!("{}", "Timing".bold());
    if let Some(ref started) = job.started_at {
        eprintln!("  {} {}", "Started:".dimmed(), started);
    }
    if let Some(ref completed) = job.completed_at {
        eprintln!("  {} {}", "Completed:".dimmed(), completed);
    }
    if let Some(duration) = job.duration_seconds {
        eprintln!("  {} {}s", "Duration:".dimmed(), duration);
    }
    if let Some(ref error) = job.error_message {
        eprintln!();
        eprintln!("{}", "Error".bold().red());
        eprintln!("  {}", error.red());
    }
    eprintln!();
}

fn print_event(event: &serde_json::Value) {
    let timestamp = chrono::Utc::now().format("%H:%M:%S").to_string();

    if let Some(event_type) = event.get("type").and_then(|v| v.as_str()) {
        let icon = match event_type {
            "PageCrawled" => "📄",
            "PageFailed" => "❌",
            "UrlsDiscovered" => "🔗",
            "JobStarted" => "🚀",
            "JobCompleted" => "✅",
            "JobFailed" => "💥",
            _ => "📌",
        };

        let message = match event_type {
            "PageCrawled" => {
                let url = event
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let status = event.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
                format!("Crawled {} ({})", url.cyan(), status)
            }
            "PageFailed" => {
                let url = event
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let error = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                format!("Failed {} - {}", url.red(), error)
            }
            "UrlsDiscovered" => {
                let count = event.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                let source = event
                    .get("source_url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!(
                    "Discovered {} URLs from {}",
                    count.to_string().green(),
                    source
                )
            }
            "JobStarted" => {
                let job_id = event
                    .get("job_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!("Job {} started", job_id.cyan())
            }
            "JobCompleted" => {
                let pages = event
                    .get("pages_crawled")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                format!(
                    "Job completed - {} pages crawled",
                    pages.to_string().green()
                )
            }
            "JobFailed" => {
                let error = event
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown error");
                format!("Job failed: {}", error.red())
            }
            _ => format!("{:?}", event),
        };

        eprintln!("{} {} {}", timestamp.dimmed(), icon, message);
    } else {
        eprintln!("{} {:?}", timestamp.dimmed(), event);
    }
}
