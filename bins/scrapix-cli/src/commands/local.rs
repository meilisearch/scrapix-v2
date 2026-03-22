use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use tokio::sync::Mutex;
use tracing::debug;

use scrapix_core::CrawlConfig;
use scrapix_extractor::Extractor;
use scrapix_parser::{extract_content, html_to_markdown};

use crate::output::{print_info, print_json, print_success};
use crate::types::{LocalCrawlDocument, LocalCrawlResult};

pub async fn handle_local(
    config_path: Option<String>,
    config_json: Option<String>,
    output: Option<String>,
    concurrency: usize,
    verbose: bool,
    json: bool,
) -> Result<()> {
    if verbose {
        let _ = tracing_subscriber::fmt()
            .with_env_filter("scrapix=debug,info")
            .try_init();
    }

    let config: CrawlConfig = if let Some(path) = config_path {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path))?;
        serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path))?
    } else if let Some(json_str) = config_json {
        serde_json::from_str(&json_str).context("Failed to parse inline config")?
    } else {
        anyhow::bail!("Either config file path or --inline JSON required");
    };

    if config.start_urls.is_empty() {
        anyhow::bail!("Configuration must include at least one start_url");
    }

    if !json {
        print_info(&format!(
            "Starting local crawl of {} URL(s)",
            config.start_urls.len()
        ));
    }

    let start_time = std::time::Instant::now();

    let http_client = reqwest::Client::builder()
        .user_agent("Scrapix/1.0 (Local Crawl)")
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let feature_extractor = Arc::new(Extractor::with_all_features());
    let visited: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let queue: Arc<Mutex<VecDeque<(String, u32)>>> = Arc::new(Mutex::new(VecDeque::new()));
    let documents: Arc<Mutex<Vec<LocalCrawlDocument>>> = Arc::new(Mutex::new(Vec::new()));
    let pages_crawled = Arc::new(AtomicU64::new(0));
    let pages_failed = Arc::new(AtomicU64::new(0));

    {
        let mut q = queue.lock().await;
        let mut v = visited.lock().await;
        for url in &config.start_urls {
            if let Ok(parsed) = url::Url::parse(url) {
                let normalized = parsed.to_string();
                if v.insert(normalized.clone()) {
                    q.push_back((normalized, 0));
                }
            }
        }
    }

    let max_depth = config.max_depth.unwrap_or(2);
    let max_pages = config.max_pages.unwrap_or(100);

    let base_domains: HashSet<String> = config
        .start_urls
        .iter()
        .filter_map(|u| url::Url::parse(u).ok())
        .filter_map(|u| u.host_str().map(|h| h.to_string()))
        .collect();

    let progress = if !json {
        let pb = ProgressBar::new(max_pages);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} pages ({msg})")
                .unwrap()
                .progress_chars("##-"),
        );
        Some(pb)
    } else {
        None
    };

    let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrency));

    loop {
        let current_pages = pages_crawled.load(Ordering::Relaxed);
        if current_pages >= max_pages {
            break;
        }

        let next = {
            let mut q = queue.lock().await;
            q.pop_front()
        };

        let Some((url, depth)) = next else {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let q = queue.lock().await;
            if q.is_empty() {
                break;
            }
            continue;
        };

        if depth > max_depth {
            continue;
        }

        let permit = semaphore.clone().acquire_owned().await?;
        let http_client = http_client.clone();
        let feature_extractor = feature_extractor.clone();
        let visited = visited.clone();
        let queue = queue.clone();
        let documents = documents.clone();
        let pages_crawled = pages_crawled.clone();
        let pages_failed = pages_failed.clone();
        let base_domains = base_domains.clone();
        let progress = progress.clone();

        tokio::spawn(async move {
            let _permit = permit;
            debug!(url = %url, depth, "Fetching");

            match http_client.get(&url).send().await {
                Ok(response) => {
                    let status_code = response.status().as_u16();
                    match response.text().await {
                        Ok(html) => {
                            let content = extract_content(&html);
                            let features = feature_extractor.extract(&html).ok();
                            let markdown = html_to_markdown(&html);

                            let title = features
                                .as_ref()
                                .and_then(|f| f.metadata.as_ref())
                                .and_then(|m| m.title.clone());
                            let description = features
                                .as_ref()
                                .and_then(|f| f.metadata.as_ref())
                                .and_then(|m| m.description.clone());

                            let doc = LocalCrawlDocument {
                                url: url.clone(),
                                title,
                                description,
                                content,
                                markdown: Some(markdown),
                                crawled_at: chrono::Utc::now().to_rfc3339(),
                                status_code,
                                depth,
                            };

                            documents.lock().await.push(doc);
                            pages_crawled.fetch_add(1, Ordering::Relaxed);

                            let new_urls = extract_urls_from_html(&html, &url, &base_domains);
                            queue_urls(new_urls, depth, &visited, &queue).await;

                            if let Some(ref pb) = progress {
                                pb.set_position(pages_crawled.load(Ordering::Relaxed));
                                pb.set_message(format!(
                                    "{} errors",
                                    pages_failed.load(Ordering::Relaxed)
                                ));
                            }
                        }
                        Err(e) => {
                            debug!(url = %url, error = %e, "Failed to read response body");
                            pages_failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Err(e) => {
                    debug!(url = %url, error = %e, "Fetch failed");
                    pages_failed.fetch_add(1, Ordering::Relaxed);

                    if let Some(ref pb) = progress {
                        pb.set_message(format!("{} errors", pages_failed.load(Ordering::Relaxed)));
                    }
                }
            }
        });
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    if let Some(pb) = progress {
        pb.finish_with_message("done");
    }

    let duration = start_time.elapsed();

    let docs = documents.lock().await.clone();
    let result = LocalCrawlResult {
        index_uid: config.index_uid.clone(),
        pages_crawled: pages_crawled.load(Ordering::Relaxed),
        pages_failed: pages_failed.load(Ordering::Relaxed),
        duration_seconds: duration.as_secs_f64(),
        documents: docs,
    };

    if let Some(output_path) = output {
        let json_str = serde_json::to_string_pretty(&result)?;
        std::fs::write(&output_path, json_str)?;

        if !json {
            print_success(&format!("Results written to {}", output_path.cyan()));
        }
    }

    if json {
        print_json(&result);
    } else {
        eprintln!();
        eprintln!("{}", "Crawl Summary".bold().underline());
        eprintln!();
        eprintln!(
            "  {} {}",
            "Pages Crawled:".dimmed(),
            result.pages_crawled.to_string().green()
        );
        eprintln!(
            "  {} {}",
            "Pages Failed:".dimmed(),
            if result.pages_failed > 0 {
                result.pages_failed.to_string().red().to_string()
            } else {
                result.pages_failed.to_string()
            }
        );
        eprintln!("  {} {:.2}s", "Duration:".dimmed(), result.duration_seconds);
        eprintln!(
            "  {} {:.2}/s",
            "Rate:".dimmed(),
            result.pages_crawled as f64 / result.duration_seconds
        );
        eprintln!();
    }

    Ok(())
}

fn extract_urls_from_html(
    html: &str,
    base_url: &str,
    base_domains: &HashSet<String>,
) -> Vec<String> {
    use scraper::{Html, Selector};

    let Ok(base) = url::Url::parse(base_url) else {
        return vec![];
    };

    let document = Html::parse_document(html);
    let Ok(selector) = Selector::parse("a[href]") else {
        return vec![];
    };

    let mut urls = Vec::new();
    for element in document.select(&selector) {
        if let Some(href) = element.value().attr("href") {
            if let Ok(resolved) = base.join(href) {
                if let Some(host) = resolved.host_str() {
                    if base_domains.contains(host) {
                        urls.push(resolved.to_string());
                    }
                }
            }
        }
    }
    urls
}

async fn queue_urls(
    urls: Vec<String>,
    depth: u32,
    visited: &Arc<Mutex<HashSet<String>>>,
    queue: &Arc<Mutex<VecDeque<(String, u32)>>>,
) {
    let mut q = queue.lock().await;
    let mut v = visited.lock().await;

    for url_str in urls {
        if v.insert(url_str.clone()) {
            q.push_back((url_str, depth + 1));
        }
    }
}
