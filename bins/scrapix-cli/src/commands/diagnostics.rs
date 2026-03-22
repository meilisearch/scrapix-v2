use anyhow::Result;
use colored::Colorize;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{print_info, print_json, truncate_url};
use crate::types::{
    DomainRow, DomainsResponse, ErrorsResponse, HealthResponse, SystemStatsResponse,
};

pub async fn handle_health(client: &ApiClient, json: bool) -> Result<()> {
    let health: HealthResponse = client.get("/health").await?;

    if json {
        print_json(&health);
    } else {
        eprintln!();
        eprintln!("{}", "API Server Health".bold().underline());
        eprintln!();
        eprintln!(
            "  {} {}",
            "Status:".dimmed(),
            if health.status == "ok" {
                health.status.green()
            } else {
                health.status.red()
            }
        );
        eprintln!("  {} {}", "Version:".dimmed(), health.version);
        eprintln!(
            "  {} {}",
            "Kafka:".dimmed(),
            if health.kafka_connected {
                "connected".green()
            } else {
                "disconnected".red()
            }
        );
        eprintln!();
    }
    Ok(())
}

pub async fn handle_stats(client: &ApiClient, json: bool) -> Result<()> {
    let stats: SystemStatsResponse = client.get("/stats").await?;

    if json {
        print_json(&stats);
    } else {
        eprintln!();
        eprintln!("{}", "System Statistics".bold().underline());
        eprintln!();

        eprintln!("{}", "Meilisearch".bold());
        if let Some(ms) = &stats.meilisearch {
            eprintln!(
                "  {} {}",
                "Status:".dimmed(),
                if ms.available {
                    "connected".green()
                } else {
                    "unavailable".red()
                }
            );
            eprintln!("  {} {}", "URL:".dimmed(), ms.url);
        } else {
            eprintln!("  {} {}", "Status:".dimmed(), "not configured".yellow());
        }

        eprintln!();
        eprintln!("{}", "Jobs".bold());
        eprintln!(
            "  {} {} total ({} running, {} completed, {} failed, {} pending)",
            "Summary:".dimmed(),
            stats.jobs.total,
            stats.jobs.running.to_string().yellow(),
            stats.jobs.completed.to_string().green(),
            stats.jobs.failed.to_string().red(),
            stats.jobs.pending
        );

        eprintln!();
        eprintln!("{}", "Diagnostics".bold());
        eprintln!(
            "  {} {}",
            "Tracked Domains:".dimmed(),
            stats.diagnostics.tracked_domains
        );
        eprintln!(
            "  {} {}",
            "Total Requests:".dimmed(),
            stats.diagnostics.total_requests
        );
        let success_rate = if stats.diagnostics.total_requests > 0 {
            (stats.diagnostics.total_successes as f64 / stats.diagnostics.total_requests as f64
                * 100.0) as u32
        } else {
            0
        };
        eprintln!(
            "  {} {}% ({} successes, {} failures)",
            "Success Rate:".dimmed(),
            success_rate,
            stats.diagnostics.total_successes.to_string().green(),
            stats.diagnostics.total_failures.to_string().red()
        );
        eprintln!(
            "  {} {}",
            "Recent Errors:".dimmed(),
            stats.diagnostics.recent_errors_count
        );
        eprintln!();
        eprintln!(
            "{}",
            format!("Collected at: {}", stats.collected_at).dimmed()
        );
        eprintln!();
    }
    Ok(())
}

pub async fn handle_errors(
    client: &ApiClient,
    last: usize,
    job_id: Option<String>,
    json: bool,
) -> Result<()> {
    let mut path = format!("/errors?last={}", last);
    if let Some(ref job) = job_id {
        path.push_str(&format!("&job_id={}", job));
    }

    let errors: ErrorsResponse = client.get(&path).await?;

    if json {
        print_json(&errors);
    } else {
        if errors.errors.is_empty() {
            print_info("No errors found");
            return Ok(());
        }

        eprintln!();
        eprintln!(
            "{} ({} total)",
            "Recent Errors".bold().underline(),
            errors.total_count
        );
        eprintln!();

        if !errors.by_status.is_empty() {
            eprintln!("{}", "By Status Code:".bold());
            let mut codes: Vec<_> = errors.by_status.iter().collect();
            codes.sort_by_key(|(k, _)| k.parse::<u16>().unwrap_or(0));
            for (code, count) in codes {
                let color = if code.starts_with('5') {
                    "red"
                } else {
                    "yellow"
                };
                eprintln!("  {} {}", code.color(color), count);
            }
            eprintln!();
        }

        if !errors.by_domain.is_empty() {
            eprintln!("{}", "Top Domains:".bold());
            for (domain, count) in errors.by_domain.iter().take(5) {
                eprintln!("  {} {}", domain.cyan(), count);
            }
            eprintln!();
        }

        eprintln!("{}", "Errors:".bold());
        for err in &errors.errors {
            let timestamp = if err.timestamp.len() > 19 {
                &err.timestamp[11..19]
            } else {
                &err.timestamp
            };
            let status = err
                .status_code
                .map(|s| s.to_string())
                .unwrap_or_else(|| "---".to_string());

            eprintln!(
                "{} {} {} {}",
                timestamp.dimmed(),
                status.red(),
                err.domain.cyan(),
                truncate_url(&err.url, 50)
            );
            eprintln!("  {} {}", "Error:".dimmed(), err.error);
        }

        eprintln!();
        eprintln!(
            "{}",
            format!("Source: {} (recent only)", errors.source).dimmed()
        );
        eprintln!();
    }
    Ok(())
}

pub async fn handle_domains(
    client: &ApiClient,
    top: usize,
    filter: Option<String>,
    json: bool,
) -> Result<()> {
    let mut path = format!("/domains?top={}", top);
    if let Some(ref f) = filter {
        path.push_str(&format!("&filter={}", urlencoding::encode(f)));
    }

    let domains: DomainsResponse = client.get(&path).await?;

    if json {
        print_json(&domains);
    } else {
        if domains.domains.is_empty() {
            print_info("No domain data found");
            return Ok(());
        }

        let rows: Vec<DomainRow> = domains
            .domains
            .iter()
            .map(|d| {
                let success_rate = if d.total_requests > 0 {
                    (d.successful_requests as f64 / d.total_requests as f64 * 100.0) as u32
                } else {
                    0
                };

                DomainRow {
                    domain: if d.domain.len() > 30 {
                        format!("{}...", &d.domain[..27])
                    } else {
                        d.domain.clone()
                    },
                    requests: d.total_requests,
                    success: format!("{}%", success_rate),
                    failed: d.failed_requests,
                    avg_time: d
                        .avg_response_time_ms
                        .map(|t| format!("{:.0}ms", t))
                        .unwrap_or_else(|| "-".to_string()),
                }
            })
            .collect();

        println!("{}", Table::new(rows));

        eprintln!();
        eprintln!("{}", format!("Source: {}", domains.source).dimmed());
    }
    Ok(())
}
