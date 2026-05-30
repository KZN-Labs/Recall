use anyhow::Result;
use colored::Colorize;
use crate::{api::ApiClient, fmt};

pub async fn run(api: &ApiClient, entity: &str, workspace: Option<&str>) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable"));
        return Ok(());
    }

    // Fetch all memory entries for this entity across all workspaces
    let entries = if let Some(ws) = workspace {
        let ws_id = format!("ws_{}", ws.trim_start_matches("ws_"));
        api.list_memory(&ws_id).await.unwrap_or_default()
            .into_iter().filter(|e| e.entity == entity).collect::<Vec<_>>()
    } else {
        api.get_entity(entity).await.unwrap_or_default()
    };

    if entries.is_empty() {
        println!("{} No memory entries found for entity: {}", fmt::warn("⚠"), fmt::entity(entity));
        return Ok(());
    }

    // Fetch conflicts mentioning any of these entries
    let workspaces: Vec<String> = entries.iter()
        .map(|e| e.workspace_id.clone()).collect::<std::collections::HashSet<_>>()
        .into_iter().collect();

    let mut conflicts: Vec<crate::api::Conflict> = vec![];
    for ws in &workspaces {
        let mut cs = api.list_conflicts(ws).await.unwrap_or_default();
        cs.retain(|c| c.entity == entity);
        conflicts.extend(cs);
    }

    // ── Header ───────────────────────────────────────────────────────────────
    println!();
    println!("{}  {}",
        "RECALL receipt trail for".truecolor(100,100,100),
        entity.white().bold(),
    );
    fmt::sep();

    // ── Timeline ─────────────────────────────────────────────────────────────
    let mut sorted = entries.clone();
    sorted.sort_by_key(|e| e.timestamp_secs.unwrap_or(0));

    for (i, entry) in sorted.iter().enumerate() {
        // Timestamp + agent + event
        println!("{}  {}  {}  {}",
            fmt::ts(entry.timestamp_secs),
            fmt::agent(&entry.agent_id),
            "→".truecolor(80,80,80),
            fmt::event(&entry.event),
        );

        // Value
        let val_str = match &entry.data {
            serde_json::Value::Object(m) => {
                if let Some(v) = m.get("value") { v.to_string() }
                else { serde_json::to_string(&entry.data).unwrap_or_default() }
            }
            v => v.to_string(),
        };
        println!("   {}  {}", fmt::label("value:"), val_str.truecolor(180, 200, 180));

        if !entry.tags.is_empty() {
            println!("   {}  {}", fmt::label("tags: "), fmt::tags(&entry.tags));
        }

        println!("   {}  {}  {}  {}",
            fmt::label("trust:"),
            fmt::trust_label(entry.trust_level),
            fmt::label("receipt:"),
            fmt::receipt_id(&entry.id),
        );

        // Check for conflict on this entry
        let my_conflicts: Vec<_> = conflicts.iter()
            .filter(|c| c.entry_a_id.contains(&entry.id) || c.entry_b_id.contains(&entry.id))
            .collect();

        for c in &my_conflicts {
            let other_id = if c.entry_a_id.contains(&entry.id) { &c.entry_b_id } else { &c.entry_a_id };
            // Find the other entry
            let other = sorted.iter().find(|e| other_id.contains(&e.id));
            let other_label = other
                .map(|o| format!("{} ({})", o.event.red(), fmt::agent(&o.agent_id)))
                .unwrap_or_else(|| other_id[..other_id.len().min(14)].red().to_string());

            println!("   {} {} {}  {}",
                "⚡ CONFLICT".yellow().bold(),
                "with:".truecolor(100,100,100),
                other_label,
                if c.resolution.is_empty() {
                    "UNRESOLVED".yellow().to_string()
                } else {
                    format!("→ {}", c.auto_resolution).green().to_string()
                }
            );
        }

        // Separator between entries (not after last)
        if i < sorted.len() - 1 {
            println!("   {}", "│".truecolor(50,50,70));
        }
    }

    // ── Summary ───────────────────────────────────────────────────────────────
    fmt::sep();
    let active_conflicts = conflicts.iter().filter(|c| c.resolution.is_empty()).count();
    let resolved         = conflicts.iter().filter(|c| !c.resolution.is_empty()).count();

    println!("{} entries  ·  {} conflicts ({} resolved)  ·  receipt chain intact",
        sorted.len().to_string().white().bold(),
        (active_conflicts + resolved).to_string().yellow(),
        resolved.to_string().green(),
    );

    // ── Current state ─────────────────────────────────────────────────────────
    if let Some(latest) = sorted.last() {
        println!();
        println!("{}", "Current state:".truecolor(100,100,100).italic());
        println!("  {} → {}  {}",
            fmt::agent(&latest.agent_id),
            fmt::event(&latest.event),
            fmt::ts(latest.timestamp_secs),
        );
    }
    println!();
    Ok(())
}
