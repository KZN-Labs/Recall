use anyhow::Result;
use colored::Colorize;
use crate::{api::ApiClient, fmt};

pub async fn run(api: &ApiClient, workspace: Option<&str>) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable"));
        return Ok(());
    }

    let workspaces: Vec<String> = if let Some(ws) = workspace {
        vec![format!("ws_{}", ws.trim_start_matches("ws_"))]
    } else {
        api.list_workspaces().await?.into_iter().map(|w| w.workspace_id).collect()
    };

    if workspaces.is_empty() {
        println!("{}", fmt::dim("No workspaces found."));
        return Ok(());
    }

    println!("\n{:<24}  {:<22}  {:<8}  {:<12}  {:<12}  {:<10}  {}",
        "AGENT".truecolor(80,80,80),
        "WORKSPACE".truecolor(80,80,80),
        "TRUST".truecolor(80,80,80),
        "ROLE".truecolor(80,80,80),
        "STAGE".truecolor(80,80,80),
        "REP".truecolor(80,80,80),
        "WRITES".truecolor(80,80,80),
    );
    fmt::sep();

    let mut total = 0usize;
    for ws in &workspaces {
        let agents = api.workspace_agents(ws).await.unwrap_or_default();
        for a in &agents {
            let last_write_ts = "-";
            println!("{:<24}  {:<22}  {:<8}  {:<12}  {:<12}  {:>6.0}%  {:>6}",
                fmt::agent(&a.agent_id),
                fmt::workspace_badge(ws),
                fmt::trust_label(a.trust_level),
                a.role.truecolor(140,140,140).to_string(),
                fmt::stage_label(&a.stage),
                a.reputation,
                a.write_count.to_string().truecolor(140,140,140),
            );
            let _ = last_write_ts;
            total += 1;
        }
    }

    fmt::sep();
    println!("{} agents across {} workspace{}",
        total.to_string().white().bold(),
        workspaces.len(),
        if workspaces.len() == 1 { "" } else { "s" },
    );
    println!();
    Ok(())
}
