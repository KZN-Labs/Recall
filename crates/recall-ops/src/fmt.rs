/// Terminal output formatting — colours, tables, separators.
use colored::Colorize;

pub fn ts(secs: Option<i64>) -> String {
    match secs {
        None => "—".dimmed().to_string(),
        Some(s) => {
            let dt = chrono::DateTime::from_timestamp(s, 0)
                .map(|d: chrono::DateTime<chrono::Utc>|
                    d.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| s.to_string());
            dt.truecolor(100, 100, 100).to_string()
        }
    }
}

pub fn agent(id: &str) -> String {
    id.cyan().to_string()
}

pub fn entity(id: &str) -> String {
    id.white().bold().to_string()
}

pub fn event(name: &str) -> String {
    name.bright_white().to_string()
}

pub fn blob(id: &str) -> String {
    if id.is_empty() { "—".truecolor(80,80,80).to_string() }
    else { id.truecolor(140, 140, 140).to_string() }
}

pub fn receipt_id(id: &str) -> String {
    format!("#{}", &id[..id.len().min(14)]).truecolor(90, 90, 160).to_string()
}

#[allow(dead_code)]
pub fn conflict_id(id: &str) -> String {
    format!("⚠  {}", &id[..id.len().min(14)]).yellow().to_string()
}

pub fn ok(msg: &str) -> String { msg.green().to_string() }
pub fn err(msg: &str) -> String { msg.red().to_string() }
pub fn warn(msg: &str) -> String { msg.yellow().to_string() }
pub fn dim(msg: &str) -> String { msg.truecolor(90,90,90).to_string() }
pub fn label(msg: &str) -> String { msg.truecolor(120,120,120).to_string() }

pub fn sep() { println!("{}", "─".repeat(72).truecolor(40,40,60)); }
pub fn thin_sep() { println!("{}", dim("·".repeat(72).as_str())); }

pub fn workspace_badge(ws: &str) -> String {
    let name = ws.trim_start_matches("ws_");
    format!("[{}]", name).truecolor(80, 120, 200).to_string()
}

pub fn trust_label(level: i32) -> String {
    match level {
        1 => "LOW".yellow().to_string(),
        2 => "MED".blue().to_string(),
        3 => "HIGH".green().to_string(),
        _ => level.to_string().dimmed().to_string(),
    }
}

pub fn stage_label(stage: &str) -> String {
    match stage {
        "NONE"       => stage.green().to_string(),
        "DETECT"     => stage.yellow().to_string(),
        "COACH"      => stage.truecolor(255,140,0).to_string(),
        "QUARANTINE" => stage.red().to_string(),
        "EVICT"      => stage.bright_red().bold().to_string(),
        _            => stage.dimmed().to_string(),
    }
}

pub fn tags(tags: &[String]) -> String {
    if tags.is_empty() { return dim("—"); }
    tags.iter()
        .map(|t| format!("{}", t.truecolor(100, 160, 100)))
        .collect::<Vec<_>>().join(" ")
}
