use anyhow::Result;
use colored::Colorize;
use crate::{api::ApiClient, fmt};

pub async fn run(
    api: &ApiClient,
    workspace: Option<&str>,
    unresolved_only: bool,
) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable"));
        return Ok(());
    }

    // ── Conflicts ────────────────────────────────────────────────────────────
    let conflicts = if let Some(ws) = workspace {
        let ws_id = format!("ws_{}", ws.trim_start_matches("ws_"));
        api.list_conflicts(&ws_id).await.unwrap_or_default()
    } else {
        api.all_conflicts().await.unwrap_or_default()
    };

    let filtered: Vec<_> = if unresolved_only {
        conflicts.iter().filter(|c| c.resolution.is_empty()).collect()
    } else {
        conflicts.iter().collect()
    };

    // ── Denied receipts ──────────────────────────────────────────────────────
    let denied: Vec<crate::api::Receipt> = if let Some(ws) = workspace {
        let ws_id = format!("ws_{}", ws.trim_start_matches("ws_"));
        api.list_receipts(&ws_id, Some("governance.check.deny"))
            .await.unwrap_or_default()
    } else {
        let workspaces = api.list_workspaces().await.unwrap_or_default();
        let mut all = vec![];
        for w in workspaces {
            let mut r = api.list_receipts(&w.workspace_id, Some("governance.check.deny"))
                .await.unwrap_or_default();
            all.append(&mut r);
        }
        all
    };

    if filtered.is_empty() && denied.is_empty() {
        println!("{}", fmt::ok("✓ No failures or conflicts found."));
        return Ok(());
    }

    // ── Print conflicts ──────────────────────────────────────────────────────
    if !filtered.is_empty() {
        println!("\n{}", "CONFLICTS".truecolor(200,160,0).bold());
        fmt::sep();
        println!("{:<20}  {:<16}  {:<42}  {:<24}  {}",
            "TIMESTAMP".truecolor(80,80,80),
            "CONFLICT ID".truecolor(80,80,80),
            "SIGNALS".truecolor(80,80,80),
            "ENTITY".truecolor(80,80,80),
            "STATUS".truecolor(80,80,80),
        );
        fmt::thin_sep();
        for c in &filtered {
            let signal_a = c.entry_a_id.chars().take(18).collect::<String>();
            let signal_b = c.entry_b_id.chars().take(18).collect::<String>();
            let signals = format!("{}  vs  {}", signal_a.cyan(), signal_b.red());
            let status = if c.resolution.is_empty() {
                "⚠  UNRESOLVED".yellow().to_string()
            } else {
                format!("✓ {}", c.resolution).green().to_string()
            };
            println!("{:<20}  {:<16}  {:<60}  {:<24}  {}",
                fmt::dim("—"),
                fmt::warn(&c.conflict_id[..c.conflict_id.len().min(14)]),
                signals,
                fmt::entity(&c.entity),
                status,
            );
        }
    }

    // ── Print denied writes ──────────────────────────────────────────────────
    if !denied.is_empty() {
        println!("\n{}", "DENIED WRITES".truecolor(200,100,0).bold());
        fmt::sep();
        println!("{:<20}  {:<20}  {:<50}",
            "TIMESTAMP".truecolor(80,80,80),
            "AGENT".truecolor(80,80,80),
            "REASON".truecolor(80,80,80),
        );
        fmt::thin_sep();
        for r in &denied {
            println!("{:<20}  {:<20}  {}",
                fmt::ts(r.timestamp_secs),
                fmt::agent(&r.actor_agent_id),
                r.deny_reason.as_deref().unwrap_or("governance deny").yellow(),
            );
        }
    }

    println!();
    Ok(())
}
