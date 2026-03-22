use anyhow::Result;
use colored::Colorize;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{format_bytes, print_info, print_json};
use crate::types::{
    AnalyticsResponse, ErrorDistData, ErrorDistRow, HourlyRow, HourlyStatsData, JobStatsData,
    KpisData, PipeInfo, TopDomainAnalyticsRow, TopDomainData,
};

pub async fn handle_analytics_pipes(client: &ApiClient, json: bool) -> Result<()> {
    let pipes: Vec<PipeInfo> = client.get("/analytics/v0/pipes").await?;

    if json {
        print_json(&pipes);
    } else {
        eprintln!();
        eprintln!("{}", "Available Analytics Pipes".bold().underline());
        eprintln!();
        for pipe in &pipes {
            eprintln!("  {} - {}", pipe.name.cyan(), pipe.description);
            eprintln!("    {}", pipe.endpoint.dimmed());
        }
        eprintln!();
    }
    Ok(())
}

pub async fn handle_analytics_kpis(client: &ApiClient, hours: u32, json: bool) -> Result<()> {
    let response: AnalyticsResponse<KpisData> = client
        .get(&format!("/analytics/v0/pipes/kpis.json?hours={}", hours))
        .await?;

    if json {
        print_json(&response);
    } else {
        if response.data.is_empty() {
            print_info("No data available");
            return Ok(());
        }

        let kpis = &response.data[0];
        eprintln!();
        eprintln!(
            "{} (last {} hours)",
            "Key Performance Indicators".bold().underline(),
            hours
        );
        eprintln!();
        eprintln!("  {} {}", "Total Crawls:".dimmed(), kpis.total_crawls);
        eprintln!(
            "  {} {}",
            "Total Bytes:".dimmed(),
            format_bytes(kpis.total_bytes)
        );
        eprintln!("  {} {}", "Unique Domains:".dimmed(), kpis.unique_domains);
        eprintln!("  {} {:.1}%", "Success Rate:".dimmed(), kpis.success_rate);
        eprintln!(
            "  {} {:.0}ms",
            "Avg Response Time:".dimmed(),
            kpis.avg_response_time_ms
        );
        eprintln!(
            "  {} {}",
            "Errors:".dimmed(),
            kpis.errors_count.to_string().red()
        );
        eprintln!();
        eprintln!(
            "{}",
            format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
        );
        eprintln!();
    }
    Ok(())
}

pub async fn handle_analytics_top_domains(
    client: &ApiClient,
    hours: u32,
    limit: u32,
    json: bool,
) -> Result<()> {
    let response: AnalyticsResponse<TopDomainData> = client
        .get(&format!(
            "/analytics/v0/pipes/top_domains.json?hours={}&limit={}",
            hours, limit
        ))
        .await?;

    if json {
        print_json(&response);
    } else {
        if response.data.is_empty() {
            print_info("No data available");
            return Ok(());
        }

        let rows: Vec<TopDomainAnalyticsRow> = response
            .data
            .iter()
            .map(|d| TopDomainAnalyticsRow {
                domain: if d.domain.len() > 35 {
                    format!("{}...", &d.domain[..32])
                } else {
                    d.domain.clone()
                },
                requests: d.total_requests,
                success: format!("{:.1}%", d.success_rate),
                failed: d.failed_requests,
                avg_time: format!("{:.0}ms", d.avg_response_time_ms),
                bytes: format_bytes(d.total_bytes),
            })
            .collect();

        println!("{}", Table::new(rows));
        eprintln!(
            "{}",
            format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
        );
    }
    Ok(())
}

pub async fn handle_analytics_domain(
    client: &ApiClient,
    domain: &str,
    hours: u32,
    json: bool,
) -> Result<()> {
    let response: AnalyticsResponse<TopDomainData> = client
        .get(&format!(
            "/analytics/v0/pipes/domain_stats.json?domain={}&hours={}",
            urlencoding::encode(domain),
            hours
        ))
        .await?;

    if json {
        print_json(&response);
    } else {
        if response.data.is_empty() {
            print_info(&format!("No data for domain: {}", domain));
            return Ok(());
        }

        let d = &response.data[0];
        eprintln!();
        eprintln!(
            "{}: {} (last {} hours)",
            "Domain Statistics".bold().underline(),
            d.domain.cyan(),
            hours
        );
        eprintln!();
        eprintln!("  {} {}", "Total Requests:".dimmed(), d.total_requests);
        eprintln!(
            "  {} {}",
            "Successful:".dimmed(),
            d.successful_requests.to_string().green()
        );
        eprintln!(
            "  {} {}",
            "Failed:".dimmed(),
            d.failed_requests.to_string().red()
        );
        eprintln!("  {} {:.1}%", "Success Rate:".dimmed(), d.success_rate);
        eprintln!(
            "  {} {:.0}ms",
            "Avg Response Time:".dimmed(),
            d.avg_response_time_ms
        );
        eprintln!(
            "  {} {}",
            "Total Bytes:".dimmed(),
            format_bytes(d.total_bytes)
        );
        eprintln!("  {} {}", "Unique URLs:".dimmed(), d.unique_urls);
        eprintln!();
        eprintln!(
            "{}",
            format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
        );
        eprintln!();
    }
    Ok(())
}

pub async fn handle_analytics_hourly(client: &ApiClient, hours: u32, json: bool) -> Result<()> {
    let response: AnalyticsResponse<HourlyStatsData> = client
        .get(&format!(
            "/analytics/v0/pipes/hourly_stats.json?hours={}",
            hours
        ))
        .await?;

    if json {
        print_json(&response);
    } else {
        if response.data.is_empty() {
            print_info("No data available");
            return Ok(());
        }

        let rows: Vec<HourlyRow> = response
            .data
            .iter()
            .map(|h| {
                let hour_display = if h.hour.len() > 16 {
                    h.hour[11..16].to_string()
                } else {
                    h.hour.clone()
                };
                HourlyRow {
                    hour: hour_display,
                    requests: h.requests,
                    success: format!("{:.1}%", h.success_rate),
                    failed: h.failures,
                    avg_time: format!("{:.0}ms", h.avg_response_time_ms),
                }
            })
            .collect();

        println!("{}", Table::new(rows));
        eprintln!(
            "{}",
            format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
        );
    }
    Ok(())
}

pub async fn handle_analytics_errors(client: &ApiClient, hours: u32, json: bool) -> Result<()> {
    let response: AnalyticsResponse<ErrorDistData> = client
        .get(&format!(
            "/analytics/v0/pipes/error_distribution.json?hours={}",
            hours
        ))
        .await?;

    if json {
        print_json(&response);
    } else {
        if response.data.is_empty() {
            print_info("No errors found");
            return Ok(());
        }

        let rows: Vec<ErrorDistRow> = response
            .data
            .iter()
            .map(|e| ErrorDistRow {
                status: e.status_code,
                count: e.count,
                percentage: format!("{:.1}%", e.percentage),
            })
            .collect();

        println!("{}", Table::new(rows));
        eprintln!(
            "{}",
            format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
        );
    }
    Ok(())
}

pub async fn handle_analytics_job(client: &ApiClient, job_id: &str, json: bool) -> Result<()> {
    let response: AnalyticsResponse<JobStatsData> = client
        .get(&format!(
            "/analytics/v0/pipes/job_stats.json?job_id={}",
            job_id
        ))
        .await?;

    if json {
        print_json(&response);
    } else {
        if response.data.is_empty() {
            print_info(&format!("No data for job: {}", job_id));
            return Ok(());
        }

        let j = &response.data[0];
        eprintln!();
        eprintln!(
            "{}: {}",
            "Job Statistics".bold().underline(),
            j.job_id.cyan()
        );
        eprintln!();
        eprintln!("  {} {}", "Total URLs:".dimmed(), j.total_urls);
        eprintln!(
            "  {} {}",
            "Successful:".dimmed(),
            j.successful_urls.to_string().green()
        );
        eprintln!(
            "  {} {}",
            "Failed:".dimmed(),
            j.failed_urls.to_string().red()
        );
        eprintln!("  {} {:.1}%", "Success Rate:".dimmed(), j.success_rate);
        eprintln!(
            "  {} {}",
            "Total Bytes:".dimmed(),
            format_bytes(j.total_bytes)
        );
        eprintln!(
            "  {} {:.0}ms",
            "Avg Response Time:".dimmed(),
            j.avg_response_time_ms
        );
        eprintln!("  {} {}", "Unique Domains:".dimmed(), j.unique_domains);
        eprintln!("  {} {}", "Started At:".dimmed(), j.started_at);
        eprintln!("  {} {}", "Last Activity:".dimmed(), j.last_activity_at);
        eprintln!("  {} {}s", "Duration:".dimmed(), j.duration_seconds);
        eprintln!();
        eprintln!(
            "{}",
            format!("Query time: {:.3}s", response.statistics.elapsed).dimmed()
        );
        eprintln!();
    }
    Ok(())
}
