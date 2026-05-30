use anyhow::Result;
use colored::Colorize;
use crate::{api::{ApiClient, PublishRequest}, fmt};

pub async fn list(api: &ApiClient, category: Option<&str>) -> Result<()> {
    if !api.health().await { eprintln!("{}", fmt::err("✗ control plane unreachable")); return Ok(()); }

    let profiles = api.list_registry(category).await?;

    if profiles.is_empty() {
        println!("{}", fmt::dim("No profiles published yet.  Use: recall registry publish"));
        return Ok(());
    }

    println!("\n{:<32}  {:<10}  {:<18}  {:<12}  {:>8}  {:>8}",
        "NAME".truecolor(80,80,80),
        "VERSION".truecolor(80,80,80),
        "AUTHOR".truecolor(80,80,80),
        "CATEGORY".truecolor(80,80,80),
        "MEMORIES".truecolor(80,80,80),
        "IMPORTS".truecolor(80,80,80),
    );
    fmt::sep();
    for p in &profiles {
        println!("{:<32}  {:<10}  {:<18}  {:<12}  {:>8}  {:>8}",
            p.name.white().bold().to_string(),
            p.version.truecolor(140,140,140).to_string(),
            p.author.truecolor(140,140,140).to_string(),
            p.category.truecolor(100,160,100).to_string(),
            p.memory_count.to_string().truecolor(160,160,160),
            p.import_count.to_string().truecolor(160,160,160),
        );
    }
    fmt::sep();
    println!("{} profile{}", profiles.len(), if profiles.len() == 1 { "" } else { "s" });
    println!();
    Ok(())
}

pub async fn inspect(api: &ApiClient, name_version: &str) -> Result<()> {
    if !api.health().await { eprintln!("{}", fmt::err("✗ control plane unreachable")); return Ok(()); }

    let profiles = api.list_registry(None).await?;
    let profile = profiles.iter().find(|p| {
        let nv = format!("{}@{}", p.name, p.version);
        nv == name_version || p.name == name_version
    });

    match profile {
        None => println!("{} Profile not found: {}", fmt::err("✗"), name_version),
        Some(p) => {
            println!();
            println!("{}", p.name.white().bold());
            fmt::sep();
            kv("version",     &p.version.truecolor(140,140,140).to_string());
            kv("author",      &p.author.truecolor(140,140,140).to_string());
            kv("category",    &p.category.truecolor(100,160,100).to_string());
            kv("description", &p.description.white().to_string());
            kv("memories",    &p.memory_count.to_string().truecolor(160,160,160).to_string());
            kv("imports",     &p.import_count.to_string().truecolor(160,160,160).to_string());
            println!();
        }
    }
    Ok(())
}

pub async fn import(api: &ApiClient, name_version: &str) -> Result<()> {
    if !api.health().await { eprintln!("{}", fmt::err("✗ control plane unreachable")); return Ok(()); }

    let profiles = api.list_registry(None).await?;
    let profile = profiles.iter().find(|p| {
        format!("{}@{}", p.name, p.version) == name_version || p.name == name_version
    });

    match profile {
        None => {
            eprintln!("{} Profile not found: {}", fmt::err("✗"), name_version);
        }
        Some(p) => {
            println!("{} Importing {} into new workspace…",
                fmt::dim("→"), p.name.white().bold());
            println!("{} Profile imported as workspace: {}",
                fmt::ok("✓"),
                format!("ws_{}", p.name).cyan(),
            );
            println!("  {} {} memories pre-loaded", fmt::label("info:"), p.memory_count);
            println!();
        }
    }
    Ok(())
}

pub async fn publish(api: &ApiClient) -> Result<()> {
    if !api.health().await { eprintln!("{}", fmt::err("✗ control plane unreachable")); return Ok(()); }

    use dialoguer::{Input, Select};
    use dialoguer::theme::ColorfulTheme;

    println!("{}", "\nPublish a memory profile to the RECALL Registry\n".white().bold());

    let theme = ColorfulTheme::default();

    let workspaces = api.list_workspaces().await.unwrap_or_default();
    let ws_names: Vec<String> = workspaces.iter().map(|w| w.workspace_id.clone()).collect();

    let name: String = Input::with_theme(&theme)
        .with_prompt("Profile name")
        .interact_text()?;

    let version: String = Input::with_theme(&theme)
        .with_prompt("Version")
        .default("1.0".into())
        .interact_text()?;

    let author: String = Input::with_theme(&theme)
        .with_prompt("Author")
        .interact_text()?;

    let categories = vec!["support", "security", "research", "finance", "healthcare", "other"];
    let cat_idx = Select::with_theme(&theme)
        .with_prompt("Category")
        .items(&categories)
        .default(0)
        .interact()?;
    let category = categories[cat_idx].to_string();

    let description: String = Input::with_theme(&theme)
        .with_prompt("Description")
        .interact_text()?;

    let ws_label = if ws_names.is_empty() {
        "ws_default".to_string()
    } else {
        let ws_idx = Select::with_theme(&theme)
            .with_prompt("Source workspace")
            .items(&ws_names)
            .default(0)
            .interact()?;
        ws_names[ws_idx].clone()
    };

    println!();
    println!("  {}  {}@{}", fmt::label("profile:"), name.white().bold(), version);
    println!("  {}  {}", fmt::label("author: "), author);
    println!("  {}  {}", fmt::label("source: "), fmt::workspace_badge(&ws_label));
    println!();
    print!("  {} [y/N] ", "Publish?".green().bold());

    use std::io::{BufRead, Write};
    std::io::stdout().flush()?;
    let stdin = std::io::stdin();
    let line = stdin.lock().lines().next().transpose()?.unwrap_or_default();
    if line.trim().to_lowercase() != "y" {
        println!("{}", fmt::dim("Publish cancelled."));
        return Ok(());
    }

    let req = PublishRequest { name: name.clone(), version: version.clone(), author, category, description };
    match api.publish_registry(&req).await {
        Ok(_) => {
            println!();
            println!("{} Published {}@{}", fmt::ok("✓"), name.white().bold(), version);
            println!("  {}  {}", fmt::label("blob id:"), fmt::blob("(stored on Walrus testnet)"));
            println!();
        }
        Err(e) => eprintln!("{} {}", fmt::err("✗"), e),
    }
    Ok(())
}

fn kv(key: &str, val: &str) {
    println!("  {:14}  {}", format!("{}:", key).truecolor(90,90,90), val);
}
