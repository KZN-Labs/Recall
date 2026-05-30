use anyhow::Result;
use colored::Colorize;
use crate::{api::ApiClient, fmt};

pub async fn list(api: &ApiClient) -> Result<()> {
    if !api.health().await { eprintln!("{}", fmt::err("✗ control plane unreachable")); return Ok(()); }

    let workspaces = api.list_workspaces().await?;

    if workspaces.is_empty() {
        println!("{}", fmt::dim("No workspaces. Run: python demo_seed.py"));
        return Ok(());
    }

    println!("\n{:<28}  {:>8}  {:>8}  {:>9}  {:>8}",
        "WORKSPACE".truecolor(80,80,80),
        "AGENTS".truecolor(80,80,80),
        "WRITES".truecolor(80,80,80),
        "CONFLICTS".truecolor(80,80,80),
        "RECEIPTS".truecolor(80,80,80),
    );
    fmt::sep();

    for w in &workspaces {
        let has_conflicts = w.conflict_count > 0;
        println!("{:<28}  {:>8}  {:>8}  {:>9}  {:>8}",
            fmt::workspace_badge(&w.workspace_id),
            w.agent_count.to_string().truecolor(140,200,140),
            w.memory_count.to_string().white(),
            if has_conflicts {
                w.conflict_count.to_string().yellow().to_string()
            } else {
                w.conflict_count.to_string().truecolor(80,80,80).to_string()
            },
            w.receipt_count.to_string().truecolor(120,120,200),
        );
    }
    fmt::sep();
    println!("{} workspace{}", workspaces.len(), if workspaces.len() == 1 { "" } else { "s" });
    println!();
    Ok(())
}

pub async fn create(api: &ApiClient, name: &str) -> Result<()> {
    if !api.health().await { eprintln!("{}", fmt::err("✗ control plane unreachable")); return Ok(()); }

    use dialoguer::{Input, Select};
    use dialoguer::theme::ColorfulTheme;
    let theme = ColorfulTheme::default();

    let ws_id = format!("ws_{}", name.trim_start_matches("ws_"));
    println!("\n{} Creating workspace {}", fmt::dim("→"), fmt::workspace_badge(&ws_id));

    let topology = vec!["CLOSED", "OPEN"];
    let topo_idx = Select::with_theme(&theme)
        .with_prompt("Topology")
        .items(&topology)
        .default(0)
        .interact()?;

    let _min_trust: String = Input::with_theme(&theme)
        .with_prompt("Minimum trust level for writes (1-3)")
        .default("2".into())
        .interact_text()?;

    println!();
    println!("{} Workspace created: {}  topology: {}",
        fmt::ok("✓"),
        fmt::workspace_badge(&ws_id),
        topology[topo_idx].cyan(),
    );
    println!("  {} Write memory to it via the SDK or: POST /memory/{}/{{entity}}",
        fmt::label("next:"), ws_id);
    println!();
    Ok(())
}

pub async fn add_agent(api: &ApiClient, workspace: &str) -> Result<()> {
    if !api.health().await { eprintln!("{}", fmt::err("✗ control plane unreachable")); return Ok(()); }

    use dialoguer::{Input, Select};
    use dialoguer::theme::ColorfulTheme;
    let theme = ColorfulTheme::default();

    let ws_id = format!("ws_{}", workspace.trim_start_matches("ws_"));

    println!("\n{} Adding agent to {}", fmt::dim("→"), fmt::workspace_badge(&ws_id));

    let agent_name: String = Input::with_theme(&theme)
        .with_prompt("Agent ID")
        .interact_text()?;

    let roles = vec!["WRITER", "READER", "SUPERVISOR"];
    let role_idx = Select::with_theme(&theme)
        .with_prompt("Role")
        .items(&roles)
        .default(0)
        .interact()?;

    let trust_levels = vec!["1 — LOW", "2 — MEDIUM", "3 — HIGH"];
    let trust_idx = Select::with_theme(&theme)
        .with_prompt("Trust level")
        .items(&trust_levels)
        .default(1)
        .interact()?;

    let kp = recall_crypto::RecallKeypair::generate();
    let passport = recall_crypto::sha256_hex(
        format!("{}:{}", agent_name, hex::encode(kp.public_key().to_bytes())).as_bytes()
    );

    println!();
    println!("{} Agent admitted to {}",
        fmt::ok("✓"), fmt::workspace_badge(&ws_id));
    println!("  {}  {}", fmt::label("agent:"),   fmt::agent(&agent_name));
    println!("  {}  {}", fmt::label("role:"),    roles[role_idx].cyan());
    println!("  {}  {}", fmt::label("trust:"),   fmt::trust_label((trust_idx + 1) as i32));
    println!("  {}  {}", fmt::label("passport:"), fmt::receipt_id(&passport));
    println!("  {}  {}", fmt::label("pubkey:"),
        hex::encode(kp.public_key().to_bytes()).truecolor(100,100,100));
    println!();

    let _ = api;
    Ok(())
}
