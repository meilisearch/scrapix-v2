use anyhow::Result;
use colored::Colorize;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{print_info, print_json, print_success};
use crate::types::{
    CreateEngineRequest, EngineIndex, EngineRecord, EngineRow, UpdateEngineRequest,
};

pub async fn handle_engines_list(client: &ApiClient, json: bool) -> Result<()> {
    let engines: Vec<EngineRecord> = client.get("/engines").await?;

    if json {
        print_json(&engines);
    } else {
        if engines.is_empty() {
            print_info("No engines configured");
            return Ok(());
        }

        let rows: Vec<EngineRow> = engines
            .iter()
            .map(|e| EngineRow {
                id: if e.id.len() > 8 {
                    format!("{}...", &e.id[..8])
                } else {
                    e.id.clone()
                },
                name: e.name.clone(),
                url: e.url.clone(),
                is_default: if e.is_default.unwrap_or(false) {
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

pub async fn handle_engine_show(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    let engine: EngineRecord = client.get(&format!("/engines/{}", id)).await?;

    if json {
        print_json(&engine);
    } else {
        eprintln!();
        eprintln!("{}", "Engine Details".bold().underline());
        eprintln!();
        eprintln!("  {} {}", "ID:".dimmed(), engine.id);
        eprintln!("  {} {}", "Name:".dimmed(), engine.name);
        eprintln!("  {} {}", "URL:".dimmed(), engine.url);
        eprintln!(
            "  {} {}",
            "Default:".dimmed(),
            if engine.is_default.unwrap_or(false) {
                "yes".green()
            } else {
                "no".red()
            }
        );
        eprintln!();
    }

    Ok(())
}

pub async fn handle_engine_create(
    client: &ApiClient,
    name: String,
    url: String,
    api_key: Option<String>,
    is_default: bool,
    json: bool,
) -> Result<()> {
    let request = CreateEngineRequest {
        name,
        url,
        api_key,
        is_default: if is_default { Some(true) } else { None },
    };

    let engine: EngineRecord = client.post("/engines", &request).await?;

    if json {
        print_json(&engine);
    } else {
        print_success(&format!(
            "Engine created: {} ({})",
            engine.name.cyan(),
            engine.id
        ));
    }

    Ok(())
}

pub async fn handle_engine_update(
    client: &ApiClient,
    id: &str,
    name: Option<String>,
    url: Option<String>,
    api_key: Option<String>,
    json: bool,
) -> Result<()> {
    let request = UpdateEngineRequest { name, url, api_key };
    let engine: EngineRecord = client.patch(&format!("/engines/{}", id), &request).await?;

    if json {
        print_json(&engine);
    } else {
        print_success(&format!("Engine {} updated", id.cyan()));
    }

    Ok(())
}

pub async fn handle_engine_delete(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    client.delete_no_body(&format!("/engines/{}", id)).await?;

    if json {
        print_json(&serde_json::json!({"deleted": id}));
    } else {
        print_success(&format!("Engine {} deleted", id.cyan()));
    }

    Ok(())
}

pub async fn handle_engine_default(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    let engine: EngineRecord = client
        .post(&format!("/engines/{}/default", id), &serde_json::json!({}))
        .await?;

    if json {
        print_json(&engine);
    } else {
        print_success(&format!("Engine {} set as default", engine.name.cyan()));
    }

    Ok(())
}

pub async fn handle_engine_indexes(client: &ApiClient, id: &str, json: bool) -> Result<()> {
    let indexes: Vec<EngineIndex> = client.get(&format!("/engines/{}/indexes", id)).await?;

    if json {
        print_json(&indexes);
    } else {
        if indexes.is_empty() {
            print_info("No indexes found on this engine");
            return Ok(());
        }

        for idx in &indexes {
            let docs = idx
                .number_of_documents
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".to_string());
            let pk = idx.primary_key.as_deref().unwrap_or("-");
            println!("  {} ({} docs, pk: {})", idx.uid.cyan(), docs, pk);
        }
    }

    Ok(())
}
