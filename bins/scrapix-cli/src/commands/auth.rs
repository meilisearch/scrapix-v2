use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use colored::Colorize;
use rand::Rng;
use sha2::{Digest, Sha256};
use tokio::sync::oneshot;

use crate::client::ApiClient;
use crate::config::{AuthCredential, CliConfig};
use crate::output::{create_spinner, print_error, print_info, print_json, print_success};
use crate::types::{BillingResponse, HealthResponse, WhoamiResponse};

// ============================================================================
// OAuth types
// ============================================================================

#[derive(Debug, serde::Deserialize)]
struct OAuthMetadata {
    authorization_endpoint: String,
    token_endpoint: String,
    registration_endpoint: String,
}

#[derive(Debug, serde::Deserialize)]
struct ClientRegistrationResponse {
    client_id: String,
}

#[derive(Debug, serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

// ============================================================================
// PKCE helpers
// ============================================================================

fn generate_code_verifier() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

fn compute_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hasher.finalize())
}

fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..16).map(|_| rng.gen::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

// ============================================================================
// Local callback server
// ============================================================================

async fn start_callback_server(
    expected_state: String,
) -> Result<(u16, oneshot::Receiver<(String, String)>)> {
    let (tx, rx) = oneshot::channel::<(String, String)>();
    let tx = Arc::new(tokio::sync::Mutex::new(Some(tx)));

    // Bind to a random available port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
        // Accept one connection
        if let Ok((mut stream, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};

            let mut buf = vec![0u8; 4096];
            if let Ok(n) = stream.read(&mut buf).await {
                let request = String::from_utf8_lossy(&buf[..n]);

                // Parse the GET request line to extract query params
                if let Some(path_line) = request.lines().next() {
                    if let Some(path) = path_line.split_whitespace().nth(1) {
                        let params = parse_query_params(path);

                        let code = params.get("code").cloned().unwrap_or_default();
                        let state = params.get("state").cloned().unwrap_or_default();
                        let error = params.get("error").cloned();

                        // Send success HTML response
                        let html = if error.is_some() {
                            "<html><body><h1>Login Failed</h1><p>You can close this tab.</p></body></html>"
                        } else {
                            "<html><body><h1>Login Successful</h1><p>You can close this tab and return to the terminal.</p></body></html>"
                        };

                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            html.len(),
                            html
                        );
                        let _ = stream.write_all(response.as_bytes()).await;

                        // Validate state
                        if state == expected_state && !code.is_empty() {
                            if let Some(tx) = tx.lock().await.take() {
                                let _ = tx.send((code, state));
                            }
                        }
                    }
                }
            }
        }
    });

    Ok((port, rx))
}

fn parse_query_params(path: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(query) = path.split('?').nth(1) {
        for pair in query.split('&') {
            let mut parts = pair.splitn(2, '=');
            if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                params.insert(
                    urlencoding::decode(key).unwrap_or_default().to_string(),
                    urlencoding::decode(value).unwrap_or_default().to_string(),
                );
            }
        }
    }
    params
}

// ============================================================================
// Login command
// ============================================================================

pub async fn handle_login(api_url: &str, use_api_key: bool) -> Result<()> {
    if use_api_key {
        return handle_login_api_key(api_url).await;
    }

    // Try OAuth browser login
    eprintln!();
    print_info("Logging in via browser (OAuth)...");

    match handle_login_oauth(api_url).await {
        Ok(()) => Ok(()),
        Err(e) => {
            eprintln!();
            print_error(&format!("OAuth login failed: {}", e));
            eprintln!();
            print_info("Falling back to API key login...");
            eprintln!();
            handle_login_api_key(api_url).await
        }
    }
}

async fn handle_login_api_key(api_url: &str) -> Result<()> {
    eprintln!(
        "Enter your API key (from {}/dashboard/api-keys):",
        api_url.trim_end_matches("/api").trim_end_matches('/')
    );

    let api_key: String = dialoguer::Password::new()
        .with_prompt("API key")
        .interact()?;

    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        anyhow::bail!("API key cannot be empty");
    }

    let client = ApiClient::new(api_url, Some(AuthCredential::ApiKey(api_key.clone())));
    let health: HealthResponse = client.get("/health").await?;

    if health.status != "ok" {
        anyhow::bail!("API returned unhealthy status");
    }

    let credits_msg = match client.get::<BillingResponse>("/account/billing").await {
        Ok(billing) => format!(" — {} credits remaining", billing.credits_balance),
        Err(_) => String::new(),
    };

    let mut config = CliConfig::load().unwrap_or_default();
    config.api_url = Some(api_url.to_string());
    config.api_key = Some(api_key.clone());
    // Clear any OAuth tokens when using API key
    config.access_token = None;
    config.refresh_token = None;
    config.token_expires_at = None;
    config.oauth_client_id = None;
    config.save()?;

    let masked = if api_key.len() > 12 {
        format!("{}...", &api_key[..12])
    } else {
        api_key
    };

    print_success(&format!(
        "Authenticated as {}{}",
        masked.cyan(),
        credits_msg
    ));
    Ok(())
}

async fn handle_login_oauth(api_url: &str) -> Result<()> {
    let http = reqwest::Client::new();

    // Step 1: Discover OAuth metadata
    let metadata_url = format!(
        "{}/.well-known/oauth-authorization-server",
        api_url.trim_end_matches('/')
    );
    let metadata: OAuthMetadata = http
        .get(&metadata_url)
        .send()
        .await
        .context("Failed to fetch OAuth metadata — is this a Scrapix API server?")?
        .json()
        .await
        .context("Invalid OAuth metadata response")?;

    // Step 2: Generate PKCE parameters
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    // Step 3: Start local callback server
    let (port, callback_rx) = start_callback_server(state.clone()).await?;
    let redirect_uri = format!("http://localhost:{}/callback", port);

    // Step 4: Register dynamic client
    let register_body = serde_json::json!({
        "client_name": "Scrapix CLI",
        "redirect_uris": [redirect_uri]
    });

    let client_reg: ClientRegistrationResponse = http
        .post(&metadata.registration_endpoint)
        .json(&register_body)
        .send()
        .await
        .context("Failed to register OAuth client")?
        .json()
        .await
        .context("Invalid client registration response")?;

    // Step 5: Build authorization URL and open browser
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&code_challenge={}&code_challenge_method=S256&state={}",
        metadata.authorization_endpoint,
        urlencoding::encode(&client_reg.client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&code_challenge),
        urlencoding::encode(&state),
    );

    print_info("Opening browser for login...");
    eprintln!();

    if open::that(&auth_url).is_err() {
        eprintln!("Could not open browser automatically.");
        eprintln!("Please open this URL manually:");
        eprintln!();
        eprintln!("  {}", auth_url.cyan());
        eprintln!();
    }

    let spinner = create_spinner("Waiting for browser login...");

    // Step 6: Wait for callback with authorization code
    let (code, _returned_state) =
        tokio::time::timeout(std::time::Duration::from_secs(300), callback_rx)
            .await
            .context("Login timed out (5 minutes). Try again.")?
            .context("Callback server error")?;

    spinner.finish_and_clear();

    // Step 7: Exchange authorization code for tokens
    let token_form = [
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("code_verifier", &code_verifier),
        ("redirect_uri", &redirect_uri),
        ("client_id", &client_reg.client_id),
    ];

    let token_response: TokenResponse = http
        .post(&metadata.token_endpoint)
        .form(&token_form)
        .send()
        .await
        .context("Failed to exchange authorization code")?
        .json()
        .await
        .context("Invalid token response")?;

    // Step 8: Save tokens to config
    let expires_at = token_response
        .expires_in
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    let mut config = CliConfig::load().unwrap_or_default();
    config.api_url = Some(api_url.to_string());
    config.access_token = Some(token_response.access_token.clone());
    config.refresh_token = token_response.refresh_token;
    config.token_expires_at = expires_at;
    config.oauth_client_id = Some(client_reg.client_id);
    // Clear API key — OAuth takes precedence for this login
    config.api_key = None;
    config.save()?;

    // Step 9: Verify by calling the API
    let client = ApiClient::new(
        api_url,
        Some(AuthCredential::Bearer(token_response.access_token)),
    );

    let credits_msg = match client.get::<BillingResponse>("/account/billing").await {
        Ok(billing) => format!(" — {} credits remaining", billing.credits_balance),
        Err(_) => String::new(),
    };

    print_success(&format!("Logged in via browser{}", credits_msg));
    Ok(())
}

// ============================================================================
// Token refresh
// ============================================================================

pub async fn refresh_token_if_needed(api_url: &str, config: &mut CliConfig) -> Result<bool> {
    // Nothing to refresh if no refresh token
    let refresh_token = match config.refresh_token {
        Some(ref t) => t.clone(),
        None => return Ok(false),
    };

    // Check if token is still valid
    if config.has_valid_token() {
        return Ok(false);
    }

    // Token expired — try to refresh
    let http = reqwest::Client::new();

    let metadata_url = format!(
        "{}/.well-known/oauth-authorization-server",
        api_url.trim_end_matches('/')
    );

    let metadata: OAuthMetadata = http.get(&metadata_url).send().await?.json().await?;

    let token_form = [
        ("grant_type", "refresh_token"),
        ("refresh_token", &refresh_token),
    ];

    let token_response: TokenResponse = http
        .post(&metadata.token_endpoint)
        .form(&token_form)
        .send()
        .await
        .context("Failed to refresh token")?
        .json()
        .await
        .context("Invalid token refresh response")?;

    let expires_at = token_response
        .expires_in
        .map(|secs| chrono::Utc::now().timestamp() + secs);

    config.access_token = Some(token_response.access_token);
    config.refresh_token = token_response.refresh_token.or(Some(refresh_token));
    config.token_expires_at = expires_at;
    config.save()?;

    Ok(true)
}

// ============================================================================
// Other auth commands
// ============================================================================

pub async fn handle_logout() -> Result<()> {
    let config = CliConfig::load().unwrap_or_default();

    // If we have an OAuth token, try to revoke it
    if let (Some(ref token), Some(ref api_url)) = (&config.access_token, &config.api_url) {
        let http = reqwest::Client::new();
        let metadata_url = format!(
            "{}/.well-known/oauth-authorization-server",
            api_url.trim_end_matches('/')
        );

        if let Ok(metadata) = http.get(&metadata_url).send().await {
            if let Ok(meta) = metadata.json::<OAuthMetadata>().await {
                let _ = http
                    .post(meta.registration_endpoint.replace("/register", "/revoke"))
                    .form(&[("token", token.as_str())])
                    .send()
                    .await;
            }
        }
    }

    CliConfig::clear()?;
    print_success("Logged out. Credentials removed.");
    Ok(())
}

pub async fn handle_whoami(client: &ApiClient, json: bool) -> Result<()> {
    let whoami: WhoamiResponse = client.get("/auth/me").await?;

    if json {
        print_json(&whoami);
    } else {
        eprintln!();
        eprintln!("{}", "Current User".bold().underline());
        eprintln!();
        if let Some(ref email) = whoami.email {
            eprintln!("  {} {}", "Email:".dimmed(), email);
        }
        if let Some(ref name) = whoami.name {
            eprintln!("  {} {}", "Name:".dimmed(), name);
        }
        if let Some(ref account_name) = whoami.account_name {
            eprintln!("  {} {}", "Account:".dimmed(), account_name);
        }
        if let Some(ref tier) = whoami.tier {
            eprintln!("  {} {}", "Tier:".dimmed(), tier);
        }
        if let Some(credits) = whoami.credits_balance {
            eprintln!("  {} {}", "Credits:".dimmed(), credits);
        }
        eprintln!();
    }
    Ok(())
}

pub async fn handle_status_auth(client: &ApiClient, json: bool) -> Result<()> {
    let health: HealthResponse = client.get("/health").await?;
    let billing = client.get::<BillingResponse>("/account/billing").await.ok();
    let config = CliConfig::load().unwrap_or_default();

    let auth_method = if config.api_key.is_some() {
        "api_key"
    } else if config.access_token.is_some() {
        "oauth"
    } else {
        "none"
    };

    if json {
        let result = serde_json::json!({
            "authenticated": auth_method != "none",
            "auth_method": auth_method,
            "api_url": client.base_url,
            "api_status": health.status,
            "api_version": health.version,
            "credits_balance": billing.as_ref().map(|b| b.credits_balance),
            "tier": billing.as_ref().and_then(|b| b.tier.clone()),
            "token_valid": config.has_valid_token(),
        });
        print_json(&result);
    } else {
        eprintln!();
        eprintln!("{}", "Auth Status".bold().underline());
        eprintln!();
        eprintln!("  {} {}", "API URL:".dimmed(), client.base_url);
        eprintln!(
            "  {} {}",
            "Status:".dimmed(),
            if health.status == "ok" {
                "connected".green()
            } else {
                "error".red()
            }
        );
        eprintln!("  {} {}", "Version:".dimmed(), health.version);
        eprintln!(
            "  {} {}",
            "Auth:".dimmed(),
            match auth_method {
                "api_key" => "API key".green(),
                "oauth" => "OAuth (browser login)".green(),
                _ => "not authenticated".red(),
            }
        );
        if auth_method == "oauth" {
            eprintln!(
                "  {} {}",
                "Token:".dimmed(),
                if config.has_valid_token() {
                    "valid".green()
                } else {
                    "expired (will auto-refresh)".yellow()
                }
            );
        }
        if let Some(ref b) = billing {
            if let Some(ref tier) = b.tier {
                eprintln!("  {} {}", "Tier:".dimmed(), tier);
            }
            eprintln!("  {} {}", "Credits:".dimmed(), b.credits_balance);
        }
        eprintln!();

        let config_path = CliConfig::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        if std::path::Path::new(&config_path).exists() {
            print_info(&format!("Config: {}", config_path));
        } else {
            print_error("No config file found. Run 'scrapix login' to authenticate.");
        }
    }
    Ok(())
}
