//! Browser rendering test example
//!
//! Run with:
//! ```bash
//! cargo run --example test_browser -p scrapix-crawler --features browser-cdp
//! ```

#[cfg(not(feature = "browser-cdp"))]
fn main() {
    eprintln!("This example requires the 'browser-cdp' feature.");
    eprintln!("Run with: cargo run --example test_browser -p scrapix-crawler --features browser-cdp");
    std::process::exit(1);
}

#[cfg(feature = "browser-cdp")]
use std::time::Duration;

#[cfg(feature = "browser-cdp")]
use scrapix_crawler::renderer_cdp::{CdpRendererBuilder, WaitUntil};

#[cfg(feature = "browser-cdp")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_target(false)
        .init();

    println!("=== Scrapix Browser Rendering Test ===\n");

    // Build the CDP renderer
    println!("Launching headless Chrome...");
    let renderer = CdpRendererBuilder::new()
        .headless(true)
        .no_sandbox(false)
        .viewport(1920, 1080)
        .timeout(Duration::from_secs(30))
        .wait_until(WaitUntil::NetworkIdle)
        .build()
        .await?;

    println!("Chrome launched successfully!\n");

    // Test URLs - using sites that have JavaScript content
    let test_urls = vec![
        ("https://example.com", "Simple static page"),
        ("https://httpbin.org/html", "HTTPBin HTML test"),
    ];

    for (url, description) in test_urls {
        println!("--- Testing: {} ---", description);
        println!("URL: {}", url);

        match renderer.render(url).await {
            Ok(result) => {
                println!("  Status: {}", result.status);
                println!("  Final URL: {}", result.final_url);
                println!("  HTML length: {} bytes", result.html.len());
                println!("  Render time: {:?}", result.render_duration);

                // Show a snippet of the HTML
                let preview = if result.html.len() > 200 {
                    format!("{}...", &result.html[..200])
                } else {
                    result.html.clone()
                };
                println!("  HTML preview: {}", preview.replace('\n', " ").trim());

                if !result.console_logs.is_empty() {
                    println!("  Console logs: {:?}", result.console_logs);
                }
                if !result.js_errors.is_empty() {
                    println!("  JS errors: {:?}", result.js_errors);
                }
                println!("  SUCCESS\n");
            }
            Err(e) => {
                println!("  ERROR: {}\n", e);
            }
        }
    }

    // Test with screenshot
    println!("--- Testing screenshot capture ---");
    let url = "https://example.com";
    println!("URL: {}", url);

    match renderer.render_with_screenshot(url).await {
        Ok(result) => {
            println!("  Screenshot captured!");
            if let Some(screenshot) = &result.screenshot {
                println!("  Screenshot size: {} bytes", screenshot.len());

                // Save screenshot to file
                let path = "/tmp/scrapix_test_screenshot.png";
                std::fs::write(path, screenshot)?;
                println!("  Saved to: {}", path);
            }
            println!("  SUCCESS\n");
        }
        Err(e) => {
            println!("  ERROR: {}\n", e);
        }
    }

    // Test JavaScript execution
    println!("--- Testing JavaScript execution ---");
    let url = "https://example.com";
    let script = "document.title";
    println!("URL: {}", url);
    println!("Script: {}", script);

    match renderer.execute_script(url, script).await {
        Ok(result) => {
            println!("  Result: {}", result);
            println!("  SUCCESS\n");
        }
        Err(e) => {
            println!("  ERROR: {}\n", e);
        }
    }

    println!("=== All tests completed ===");
    Ok(())
}
