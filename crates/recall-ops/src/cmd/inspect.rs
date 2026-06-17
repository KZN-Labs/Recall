use anyhow::Result;
use colored::Colorize;
use crate::{api::ApiClient, fmt};

pub async fn run(api: &ApiClient, id: &str) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable"));
        return Ok(());
    }

    // Try receipt first (hex ID, 64 chars)
    if id.len() == 64 || id.starts_with("mem_") {
        // Try as receipt
        if let Ok(r) = api.get_receipt(id).await {
            print_receipt(&r);
            return Ok(());
        }
        // Try as memory entry across all workspaces
        let workspaces = api.list_workspaces().await.unwrap_or_default();
        for ws in &workspaces {
            let entries = api.list_memory(&ws.workspace_id).await.unwrap_or_default();
            if let Some(e) = entries.iter().find(|e| e.id == id) {
                print_memory_entry(e);
                return Ok(());
            }
        }
    }

    // Try as conflict
    let workspaces = api.list_workspaces().await.unwrap_or_default();
    for ws in &workspaces {
        let conflicts = api.list_conflicts(&ws.workspace_id).await.unwrap_or_default();
        if let Some(c) = conflicts.iter().find(|c| c.conflict_id == id || c.conflict_id.starts_with(id)) {
            print_conflict(c);
            return Ok(());
        }
    }

    println!("{} ID not found: {}", fmt::err("✗"), id);
    println!("{}", fmt::dim("  Try: recall logs  to see valid IDs"));
    Ok(())
}

fn print_receipt(r: &crate::api::Receipt) {
    println!();
    println!("{}", "RECEIPT".truecolor(100,100,200).bold());
    fmt::sep();
    kv("id",          &fmt::receipt_id(&r.id));
    kv("action",      &r.action_kind.bright_white().to_string());
    kv("workspace",   &fmt::workspace_badge(&r.workspace_id));
    kv("agent",       &fmt::agent(&r.actor_agent_id));
    kv("passport",    &fmt::dim(&r.actor_passport_id));
    kv("timestamp",   &fmt::ts(r.timestamp_secs));
    kv("seal_status", &r.seal_status.to_string().truecolor(120,120,120).to_string());
    if let Some(reason) = &r.deny_reason {
        if r.action_kind == "anchor.commit" {
            // For anchor receipts the deny_reason slot carries the Sui tx
            // digest (or an UNANCHORED:<reason> marker). Render the two cases
            // distinctly so a synthetic value never looks confirmed.
            if let Some(why) = reason.strip_prefix("UNANCHORED:") {
                kv("sui_tx",
                   &format!("{} ⚠ {}",
                            "UNANCHORED".truecolor(230,90,90).bold(),
                            why.truecolor(220,140,140)));
            } else {
                kv("sui_tx", &reason.truecolor(140,200,140).to_string());
            }
        } else {
            kv("deny_reason", &reason.yellow().to_string());
        }
    }
    if r.reputation_delta != 0.0 {
        kv("rep_delta", &format!("{:+.4}", r.reputation_delta).truecolor(200,160,80).to_string());
    }
    println!();
}

fn print_memory_entry(e: &crate::api::MemoryEntry) {
    println!();
    println!("{}", "MEMORY ENTRY".truecolor(100,200,100).bold());
    fmt::sep();
    kv("id",        &fmt::receipt_id(&e.id));
    kv("entity",    &fmt::entity(&e.entity));
    kv("workspace", &fmt::workspace_badge(&e.workspace_id));
    kv("agent",     &fmt::agent(&e.agent_id));
    kv("event",     &fmt::event(&e.event));
    kv("timestamp", &fmt::ts(e.timestamp_secs));
    kv("trust",     &fmt::trust_label(e.trust_level));
    kv("scope",     &e.scope.truecolor(120,120,120).to_string());
    if !e.tags.is_empty() {
        kv("tags", &fmt::tags(&e.tags));
    }
    kv("model",  &format!("{} / {}", e.model_provider, e.model_name).truecolor(100,100,100).to_string());
    kv("data",   &serde_json::to_string_pretty(&e.data)
        .unwrap_or_default()
        .lines()
        .map(|l| format!("              {}", l.truecolor(160,200,160)))
        .collect::<Vec<_>>()
        .join("\n")
        .trim_start()
        .to_string()
    );
    println!();
}

fn print_conflict(c: &crate::api::Conflict) {
    println!();
    println!("{}", "CONFLICT".yellow().bold());
    fmt::sep();
    kv("conflict_id",  &fmt::warn(&c.conflict_id));
    kv("entity",       &fmt::entity(&c.entity));
    kv("workspace",    &fmt::workspace_badge(&c.workspace_id));
    println!();
    println!("  {}",  "Signal A".cyan());
    println!("    {}  {}", fmt::label("entry:"), fmt::receipt_id(&c.entry_a_id));
    println!();
    println!("  {}", "Signal B".red());
    println!("    {}  {}", fmt::label("entry:"), fmt::receipt_id(&c.entry_b_id));
    println!();
    kv("auto_resolution", &c.auto_resolution.green().to_string());
    if c.resolution.is_empty() {
        kv("status", &"UNRESOLVED".yellow().to_string());
    } else {
        kv("status",     &"RESOLVED".green().to_string());
        kv("resolution", &c.resolution.green().to_string());
    }
    println!();
}

fn kv(key: &str, val: &str) {
    println!("  {:14}  {}", format!("{}:", key).truecolor(90,90,90), val);
}
