use anyhow::Result;
use colored::Colorize;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{print_info, print_json};
use crate::types::{BillingResponse, TransactionRecord, TransactionRow};

pub async fn handle_billing(client: &ApiClient, json: bool) -> Result<()> {
    let billing: BillingResponse = client.get("/account/billing").await?;

    if json {
        print_json(&billing);
    } else {
        eprintln!();
        eprintln!("{}", "Billing".bold().underline());
        eprintln!();
        eprintln!(
            "  {} {}",
            "Credits:".dimmed(),
            billing.credits_balance.to_string().bold()
        );
        if let Some(ref tier) = billing.tier {
            eprintln!("  {} {}", "Tier:".dimmed(), tier);
        }
        if let Some(enabled) = billing.auto_topup_enabled {
            eprintln!(
                "  {} {}",
                "Auto Top-up:".dimmed(),
                if enabled {
                    format!(
                        "enabled ({})",
                        billing
                            .auto_topup_amount
                            .map(|a| format!("{} credits", a))
                            .unwrap_or_default()
                    )
                    .green()
                    .to_string()
                } else {
                    "disabled".to_string()
                }
            );
        }
        if let Some(limit) = billing.monthly_spend_limit {
            eprintln!("  {} {} credits", "Spend Limit:".dimmed(), limit);
        }
        eprintln!();
    }

    Ok(())
}

pub async fn handle_billing_transactions(
    client: &ApiClient,
    limit: usize,
    offset: usize,
    json: bool,
) -> Result<()> {
    let transactions: Vec<TransactionRecord> = client
        .get(&format!(
            "/account/billing/transactions?limit={}&offset={}",
            limit, offset
        ))
        .await?;

    if json {
        print_json(&transactions);
    } else {
        if transactions.is_empty() {
            print_info("No transactions found");
            return Ok(());
        }

        let rows: Vec<TransactionRow> = transactions
            .iter()
            .map(|t| {
                let date = if t.created_at.len() > 10 {
                    t.created_at[..10].to_string()
                } else {
                    t.created_at.clone()
                };

                TransactionRow {
                    date,
                    tx_type: t.tx_type.clone(),
                    amount: if t.amount >= 0 {
                        format!("+{}", t.amount)
                    } else {
                        t.amount.to_string()
                    },
                    balance: t.balance_after.to_string(),
                    description: t.description.clone().unwrap_or_default(),
                }
            })
            .collect();

        println!("{}", Table::new(rows));
    }

    Ok(())
}
