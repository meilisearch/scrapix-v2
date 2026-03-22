use anyhow::{Context, Result};
use colored::Colorize;

use crate::output::{print_error, print_info, print_success};

pub fn handle_infra_up(json: bool) -> Result<()> {
    use std::process::Command;

    if !json {
        print_info("Starting infrastructure...");
    }

    let status = Command::new("docker")
        .args([
            "compose",
            "-f",
            "docker-compose.yml",
            "-f",
            "docker-compose.dev.yml",
            "up",
            "-d",
        ])
        .status()
        .context("Failed to start infrastructure")?;

    if status.success() {
        if json {
            println!(r#"{{"status": "started"}}"#);
        } else {
            print_success("Infrastructure started");
            eprintln!();
            eprintln!("{}", "Services:".bold());
            eprintln!("  - Redpanda (Kafka):    localhost:19092");
            eprintln!("  - Meilisearch:         localhost:7700");
            eprintln!("  - DragonflyDB (Redis): localhost:6380");
            eprintln!("  - Redpanda Console:    localhost:8090");
            eprintln!();
        }
    } else {
        print_error("Failed to start infrastructure");
    }

    Ok(())
}

pub fn handle_infra_down(json: bool) -> Result<()> {
    use std::process::Command;

    if !json {
        print_info("Stopping infrastructure...");
    }

    let status = Command::new("docker")
        .args([
            "compose",
            "-f",
            "docker-compose.yml",
            "-f",
            "docker-compose.dev.yml",
            "down",
        ])
        .status()
        .context("Failed to stop infrastructure")?;

    if status.success() {
        if json {
            println!(r#"{{"status": "stopped"}}"#);
        } else {
            print_success("Infrastructure stopped");
        }
    }

    Ok(())
}

pub fn handle_infra_restart(json: bool) -> Result<()> {
    use std::process::Command;

    if !json {
        print_info("Restarting infrastructure...");
    }

    let status = Command::new("docker")
        .args([
            "compose",
            "-f",
            "docker-compose.yml",
            "-f",
            "docker-compose.dev.yml",
            "restart",
        ])
        .status()
        .context("Failed to restart infrastructure")?;

    if status.success() {
        if json {
            println!(r#"{{"status": "restarted"}}"#);
        } else {
            print_success("Infrastructure restarted");
        }
    }

    Ok(())
}

pub fn handle_infra_status() -> Result<()> {
    use std::process::Command;
    let _ = Command::new("docker").args(["compose", "ps"]).status();
    Ok(())
}

pub fn handle_infra_logs(service: Option<String>, follow: bool) -> Result<()> {
    use std::process::Command;

    let mut args = vec!["compose", "logs"];
    if follow {
        args.push("-f");
    }
    if let Some(ref svc) = service {
        args.push(svc);
    }

    let _ = Command::new("docker").args(&args).status();
    Ok(())
}

pub fn handle_infra_reset(yes: bool, json: bool) -> Result<()> {
    use std::process::Command;

    if !yes {
        eprintln!();
        eprintln!(
            "{} This will delete all data volumes",
            "WARNING:".yellow().bold()
        );
        eprint!("Are you sure? (y/N) ");
        use std::io::Write;
        std::io::stderr().flush()?;

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            print_info("Cancelled");
            return Ok(());
        }
    }

    let status = Command::new("docker")
        .args([
            "compose",
            "-f",
            "docker-compose.yml",
            "-f",
            "docker-compose.dev.yml",
            "down",
            "-v",
        ])
        .status()
        .context("Failed to reset infrastructure")?;

    if status.success() {
        if json {
            println!(r#"{{"status": "reset"}}"#);
        } else {
            print_success("Infrastructure reset");
        }
    }

    Ok(())
}
