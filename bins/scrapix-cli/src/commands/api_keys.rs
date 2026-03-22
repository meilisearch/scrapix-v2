use anyhow::Result;
use colored::Colorize;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{print_info, print_json, print_success};
use crate::types::{ApiKeyRecord, ApiKeyRow, CreateApiKeyRequest};

pub async fn handle_api_keys_list(client: &ApiClient, json: bool) -> Result<()> {
    let keys: Vec<ApiKeyRecord> = client.get("/account/api-keys").await?;

    if json {
        print_json(&keys);
    } else {
        if keys.is_empty() {
            print_info("No API keys found");
            return Ok(());
        }

        let rows: Vec<ApiKeyRow> = keys
            .iter()
            .map(|k| ApiKeyRow {
                id: if k.id.len() > 8 {
                    format!("{}...", &k.id[..8])
                } else {
                    k.id.clone()
                },
                name: k.name.clone(),
                prefix: k.prefix.clone(),
                revoked: if k.revoked.unwrap_or(false) {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            })
            .collect();

        println!("{}", Table::new(rows));
    }

    Ok(())
}

pub async fn handle_api_key_create(client: &ApiClient, name: String, json: bool) -> Result<()> {
    let request = CreateApiKeyRequest { name };
    let key: ApiKeyRecord = client.post("/account/api-keys", &request).await?;

    if json {
        print_json(&key);
    } else {
        print_success(&format!("API key created: {}", key.name.cyan()));
        if let Some(ref full_key) = key.key {
            eprintln!();
            eprintln!("  {} {}", "Key:".dimmed(), full_key.bold());
            eprintln!();
            eprintln!(
                "{}",
                "Save this key now — it won't be shown again.".yellow()
            );
        }
    }

    Ok(())
}

pub async fn handle_api_key_revoke(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    let key: ApiKeyRecord = client
        .patch(
            &format!("/account/api-keys/{}", id),
            &serde_json::json!({"revoked": true}),
        )
        .await?;

    if json {
        print_json(&key);
    } else {
        print_success(&format!("API key {} revoked", id.cyan()));
    }

    Ok(())
}
