use anyhow::Result;
use colored::Colorize;
use crate::{api::ApiClient, fmt};

pub async fn run(api: &ApiClient, workspace: &str, to: &str) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable"));
        return Ok(());
    }

    let ws_id = format!("ws_{}", workspace.trim_start_matches("ws_"));

    // Parse `to` — accept unix timestamp or ISO-8601
    let to_ts: i64 = if let Ok(n) = to.parse::<i64>() {
        n
    } else if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(to) {
        dt.timestamp()
    } else {
        eprintln!("{} invalid --to value: {}", fmt::err("✗"), to);
        eprintln!("{}", fmt::dim("  Use unix timestamp (e.g. 1716028320) or RFC-3339 (e.g. 2026-05-18T10:30:00Z)"));
        return Ok(());
    };

    // Show what will be removed
    let entries = api.list_memory(&ws_id).await.unwrap_or_default();
    let will_remove: Vec<_> = entries.iter()
        .filter(|e| e.timestamp_secs.unwrap_or(0) > to_ts)
        .collect();

    if will_remove.is_empty() {
        println!("{} Nothing to roll back — no entries after {}",
            fmt::ok("✓"), fmt::ts(Some(to_ts)));
        return Ok(());
    }

    println!();
    println!("{} The following {} entries will be {} from {}:",
        "⚠".yellow(),
        will_remove.len().to_string().yellow().bold(),
        "removed".red(),
        fmt::workspace_badge(&ws_id),
    );
    println!();
    for e in &will_remove {
        println!("  {}  {}  {}  {}",
            fmt::ts(e.timestamp_secs),
            fmt::agent(&e.agent_id),
            fmt::event(&e.event),
            fmt::entity(&e.entity),
        );
    }
    println!();
    println!("  Rolling back to: {}", fmt::ts(Some(to_ts)));
    println!();

    // Confirmation prompt
    print!("  {} [y/N] ", "Confirm rollback?".yellow().bold());
    use std::io::{self, BufRead, Write};
    io::stdout().flush()?;
    let stdin = io::stdin();
    let line = stdin.lock().lines().next().transpose()?.unwrap_or_default();
    if line.trim().to_lowercase() != "y" {
        println!("{}", fmt::dim("Rollback cancelled."));
        return Ok(());
    }

    let result = api.rollback(&ws_id, to_ts).await?;

    println!();
    println!("{} Rolled back {} entries from {}",
        fmt::ok("✓"),
        result.entries_removed.to_string().green().bold(),
        fmt::workspace_badge(&ws_id),
    );
    println!("  {}  {}", fmt::label("rollback receipt:"), fmt::receipt_id(&result.rollback_receipt_id));
    println!();
    Ok(())
}
