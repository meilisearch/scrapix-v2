use anyhow::Result;
use colored::Colorize;
use tabled::Table;

use crate::client::ApiClient;
use crate::output::{print_info, print_json, print_success};
use crate::types::{InviteMemberRequest, TeamMember, TeamMemberRow, UpdateRoleRequest};

pub async fn handle_team_list(client: &ApiClient, json: bool) -> Result<()> {
    let members: Vec<TeamMember> = client.get("/account/members").await?;

    if json {
        print_json(&members);
    } else {
        if members.is_empty() {
            print_info("No team members found");
            return Ok(());
        }

        let rows: Vec<TeamMemberRow> = members
            .iter()
            .map(|m| TeamMemberRow {
                user_id: if m.user_id.len() > 8 {
                    format!("{}...", &m.user_id[..8])
                } else {
                    m.user_id.clone()
                },
                email: m.email.clone().unwrap_or_else(|| "-".to_string()),
                name: m.name.clone().unwrap_or_else(|| "-".to_string()),
                role: m.role.clone(),
            })
            .collect();

        println!("{}", Table::new(rows));
    }

    Ok(())
}

pub async fn handle_team_invite(
    client: &ApiClient,
    email: String,
    role: Option<String>,
    json: bool,
) -> Result<()> {
    let request = InviteMemberRequest {
        email: email.clone(),
        role,
    };
    let result: serde_json::Value = client.post("/account/members/invite", &request).await?;

    if json {
        print_json(&result);
    } else {
        print_success(&format!("Invitation sent to {}", email.cyan()));
    }

    Ok(())
}

pub async fn handle_team_remove(client: &ApiClient, user_id: &str, json: bool) -> Result<()> {
    client
        .delete_no_body(&format!("/account/members/{}", user_id))
        .await?;

    if json {
        print_json(&serde_json::json!({"removed": user_id}));
    } else {
        print_success(&format!("Member {} removed", user_id.cyan()));
    }

    Ok(())
}

pub async fn handle_team_role(
    client: &ApiClient,
    user_id: &str,
    role: String,
    json: bool,
) -> Result<()> {
    let request = UpdateRoleRequest { role: role.clone() };
    let result: serde_json::Value = client
        .patch(&format!("/account/members/{}", user_id), &request)
        .await?;

    if json {
        print_json(&result);
    } else {
        print_success(&format!(
            "Member {} role changed to {}",
            user_id.cyan(),
            role.bold()
        ));
    }

    Ok(())
}
