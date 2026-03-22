use std::time::Duration;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::output::{print_error, print_info, print_success};

fn check_kubectl() -> Result<()> {
    let output = std::process::Command::new("kubectl")
        .args(["cluster-info"])
        .output();

    if output.is_err() || !output.as_ref().unwrap().status.success() {
        anyhow::bail!("Cannot connect to Kubernetes cluster. Check your kubeconfig.");
    }
    Ok(())
}

fn component_deployment(component: &str) -> Result<&str> {
    match component {
        "api" => Ok("scrapix-api"),
        "frontier" => Ok("scrapix-frontier"),
        "crawler" => Ok("scrapix-crawler"),
        "content" => Ok("scrapix-content"),
        _ => anyhow::bail!("Unknown component: {}", component),
    }
}

pub fn handle_k8s_deploy(namespace: &str, overlay: &str, json: bool) -> Result<()> {
    use std::process::Command;

    check_kubectl()?;

    let overlay_path = format!("deploy/kubernetes/overlays/{}", overlay);
    if !std::path::Path::new(&overlay_path).exists() {
        anyhow::bail!("Overlay not found: {}", overlay_path);
    }

    if !json {
        print_info(&format!(
            "Deploying to namespace '{}' (overlay: {})",
            namespace, overlay
        ));
    }

    // Create namespace
    let _ = Command::new("kubectl")
        .args([
            "create",
            "namespace",
            namespace,
            "--dry-run=client",
            "-o",
            "yaml",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()?
        .wait_with_output()
        .and_then(|output| {
            Command::new("kubectl")
                .args(["apply", "-f", "-"])
                .stdin(std::process::Stdio::piped())
                .spawn()
                .and_then(|mut child| {
                    use std::io::Write;
                    child.stdin.as_mut().unwrap().write_all(&output.stdout)?;
                    child.wait()
                })
        });

    let status = Command::new("kubectl")
        .args(["apply", "-k", &overlay_path, "-n", namespace])
        .status()
        .context("Failed to apply kustomize")?;

    if !status.success() {
        anyhow::bail!("kubectl apply failed");
    }

    if !json {
        print_info("Waiting for deployments to be ready...");
    }

    let _ = Command::new("kubectl")
        .args([
            "rollout",
            "status",
            "deployment",
            "-n",
            namespace,
            "--timeout=300s",
        ])
        .status();

    if json {
        println!(r#"{{"status": "deployed", "namespace": "{}"}}"#, namespace);
    } else {
        print_success("Deployment complete!");
    }

    Ok(())
}

pub fn handle_k8s_destroy(namespace: &str, overlay: &str, yes: bool, json: bool) -> Result<()> {
    use std::process::Command;

    check_kubectl()?;

    if !yes {
        eprintln!();
        eprintln!(
            "{} This will delete all Scrapix resources in namespace '{}'",
            "WARNING:".yellow().bold(),
            namespace
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

    let overlay_path = format!("deploy/kubernetes/overlays/{}", overlay);

    let status = Command::new("kubectl")
        .args([
            "delete",
            "-k",
            &overlay_path,
            "-n",
            namespace,
            "--ignore-not-found",
        ])
        .status()
        .context("Failed to delete resources")?;

    if json {
        println!(r#"{{"status": "destroyed"}}"#);
    } else if status.success() {
        print_success("Resources deleted");
    } else {
        print_error("Some resources may not have been deleted");
    }

    Ok(())
}

pub fn handle_k8s_status(namespace: &str, watch: bool, json: bool) -> Result<()> {
    use std::process::Command;

    check_kubectl()?;

    if watch {
        let _ = Command::new("watch")
            .args([
                "-n",
                "2",
                "kubectl",
                "get",
                "pods,svc,deployments",
                "-n",
                namespace,
                "-o",
                "wide",
            ])
            .status();
        return Ok(());
    }

    if json {
        let output = Command::new("kubectl")
            .args(["get", "pods,svc,deployments", "-n", namespace, "-o", "json"])
            .output()
            .context("Failed to get status")?;
        println!("{}", String::from_utf8_lossy(&output.stdout));
    } else {
        eprintln!("{}", "Deployments:".bold());
        let _ = Command::new("kubectl")
            .args(["get", "deployments", "-n", namespace, "-o", "wide"])
            .status();
        eprintln!();

        eprintln!("{}", "Pods:".bold());
        let _ = Command::new("kubectl")
            .args(["get", "pods", "-n", namespace, "-o", "wide"])
            .status();
        eprintln!();

        eprintln!("{}", "Services:".bold());
        let _ = Command::new("kubectl")
            .args(["get", "svc", "-n", namespace])
            .status();
        eprintln!();

        eprintln!("{}", "Resource Usage:".bold());
        let _ = Command::new("kubectl")
            .args(["top", "pods", "-n", namespace])
            .status();
        eprintln!();
    }

    Ok(())
}

pub fn handle_k8s_logs(component: &str, namespace: &str, follow: bool) -> Result<()> {
    use std::process::Command;

    check_kubectl()?;

    let mut args = vec!["logs", "-n", namespace];

    if component == "all" {
        args.extend([
            "-l",
            "app.kubernetes.io/name=scrapix",
            "--all-containers",
            "--prefix",
        ]);
    } else {
        let deployment = format!("deployment/{}", component_deployment(component)?);
        // Need to leak the string to get a &str that lives long enough
        // Use a different approach
        args.push("deployment/placeholder");
        let last = args.len() - 1;
        args[last] = Box::leak(deployment.into_boxed_str());
        args.push("--all-containers");
    }

    if follow {
        args.push("-f");
    }

    let _ = Command::new("kubectl").args(&args).status();
    Ok(())
}

pub fn handle_k8s_scale(component: &str, replicas: u32, namespace: &str, json: bool) -> Result<()> {
    use std::process::Command;

    check_kubectl()?;
    let deployment = component_deployment(component)?;

    if !json {
        print_info(&format!(
            "Scaling {} to {} replicas...",
            deployment, replicas
        ));
    }

    let status = Command::new("kubectl")
        .args([
            "scale",
            "deployment",
            deployment,
            "-n",
            namespace,
            &format!("--replicas={}", replicas),
        ])
        .status()
        .context("Failed to scale deployment")?;

    if !status.success() {
        anyhow::bail!("Scale command failed");
    }

    let _ = Command::new("kubectl")
        .args([
            "rollout",
            "status",
            "deployment",
            deployment,
            "-n",
            namespace,
            "--timeout=120s",
        ])
        .status();

    if json {
        println!(
            r#"{{"component": "{}", "replicas": {}}}"#,
            component, replicas
        );
    } else {
        print_success(&format!("Scaled {} to {} replicas", deployment, replicas));
    }

    Ok(())
}

pub fn handle_k8s_restart(component: &str, namespace: &str, json: bool) -> Result<()> {
    use std::process::Command;

    check_kubectl()?;

    let status = if component == "all" {
        Command::new("kubectl")
            .args([
                "rollout",
                "restart",
                "deployment",
                "-n",
                namespace,
                "-l",
                "app.kubernetes.io/name=scrapix",
            ])
            .status()
    } else {
        let deployment = component_deployment(component)?;
        Command::new("kubectl")
            .args([
                "rollout",
                "restart",
                "deployment",
                deployment,
                "-n",
                namespace,
            ])
            .status()
    };

    status.context("Failed to restart")?;

    let _ = Command::new("kubectl")
        .args([
            "rollout",
            "status",
            "deployment",
            "-n",
            namespace,
            "--timeout=120s",
        ])
        .status();

    if json {
        println!(r#"{{"status": "restarted", "component": "{}"}}"#, component);
    } else {
        print_success("Restart complete");
    }

    Ok(())
}

pub fn handle_k8s_port_forward(namespace: &str, json: bool) -> Result<()> {
    use std::process::Command;

    check_kubectl()?;

    // Kill existing port forwards
    let _ = Command::new("pkill")
        .args(["-f", "kubectl port-forward.*scrapix"])
        .status();

    let _ = Command::new("kubectl")
        .args([
            "port-forward",
            "-n",
            namespace,
            "svc/scrapix-api",
            "8080:8080",
        ])
        .spawn();

    let _ = Command::new("kubectl")
        .args([
            "port-forward",
            "-n",
            namespace,
            "svc/meilisearch",
            "7700:7700",
        ])
        .spawn();

    let _ = Command::new("kubectl")
        .args([
            "port-forward",
            "-n",
            namespace,
            "svc/redpanda-console",
            "8090:8080",
        ])
        .spawn();

    if json {
        println!(
            r#"{{"api": "localhost:8080", "meilisearch": "localhost:7700", "console": "localhost:8090"}}"#
        );
    } else {
        print_success("API Server: http://localhost:8080");
        print_success("Meilisearch: http://localhost:7700");
        print_success("Redpanda Console: http://localhost:8090");
        eprintln!();
        print_info("Port forwards active. Press Ctrl+C to stop.");
    }

    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}
