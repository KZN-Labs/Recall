use anyhow::Result;
use colored::Colorize;
use std::collections::HashSet;
use crate::{api::ApiClient, fmt};

pub async fn run(
    api: &ApiClient,
    workspace: Option<&str>,
    entity: Option<&str>,
    follow: bool,
) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable — is it running on :8080?"));
        return Ok(());
    }

    let workspaces: Vec<String> = if let Some(ws) = workspace {
        vec![format!("ws_{}", ws.trim_start_matches("ws_"))]
    } else {
        api.list_workspaces().await?
            .into_iter().map(|w| w.workspace_id).collect()
    };

    if workspaces.is_empty() {
        println!("{}", fmt::dim("No workspaces found. Run: python demo_seed.py"));
        return Ok(());
    }

    // Initial fetch
    let mut seen: HashSet<String> = HashSet::new();
    let mut initial: Vec<crate::api::MemoryEntry> = vec![];
    for ws in &workspaces {
        let mut entries = api.list_memory(ws).await.unwrap_or_default();
        if let Some(e) = entity {
            entries.retain(|en| en.entity.contains(e));
        }
        initial.extend(entries);
    }
    initial.sort_by_key(|e| e.timestamp_secs.unwrap_or(0));

    let to_show: Vec<_> = if follow || initial.len() <= 50 {
        initial.iter().collect()
    } else {
        initial.iter().rev().take(50).collect::<Vec<_>>().into_iter().rev().collect()
    };

    if to_show.is_empty() {
        println!("{}", fmt::dim("No memory writes yet."));
    } else {
        print_header();
        for e in &to_show {
            print_entry(e);
            seen.insert(e.id.clone());
        }
    }

    if !follow {
        return Ok(());
    }

    // Streaming poll loop
    println!("\n{} {}",
        "●".green(),
        fmt::dim("watching for new writes (ctrl-c to stop)…"));

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let mut new_entries: Vec<crate::api::MemoryEntry> = vec![];
        for ws in &workspaces {
            let mut entries = api.list_memory(ws).await.unwrap_or_default();
            if let Some(e) = entity {
                entries.retain(|en| en.entity.contains(e));
            }
            for entry in entries {
                if !seen.contains(&entry.id) {
                    new_entries.push(entry);
                }
            }
        }
        new_entries.sort_by_key(|e| e.timestamp_secs.unwrap_or(0));
        for entry in new_entries {
            if seen.insert(entry.id.clone()) {
                print_entry(&entry);
            }
        }
    }
}

fn print_header() {
    println!("{:<20}  {:<20}  {:<26}  {:<24}  {}",
        "TIMESTAMP".truecolor(80,80,80),
        "AGENT".truecolor(80,80,80),
        "EVENT".truecolor(80,80,80),
        "ENTITY".truecolor(80,80,80),
        "BLOB ID".truecolor(80,80,80),
    );
    fmt::sep();
}

fn print_entry(e: &crate::api::MemoryEntry) {
    println!("{:<20}  {:<20}  {:<26}  {:<24}  {}",
        fmt::ts(e.timestamp_secs),
        fmt::agent(&e.agent_id),
        fmt::event(&e.event),
        fmt::entity(&e.entity),
        fmt::blob(&fmt::receipt_id(&e.id)),
    );
}
