use anyhow::Result;
use colored::Colorize;
use printpdf::*;
use std::fs::File;
use std::io::BufWriter;
use crate::{api::ApiClient, fmt};

pub async fn run(
    api: &ApiClient,
    entity: &str,
    workspace: Option<&str>,
    from_ts: Option<i64>,
    to_ts: Option<i64>,
    output: Option<&str>,
) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable"));
        return Ok(());
    }

    let entries = if let Some(ws) = workspace {
        let ws_id = format!("ws_{}", ws.trim_start_matches("ws_"));
        api.list_memory(&ws_id).await.unwrap_or_default()
            .into_iter().filter(|e| e.entity == entity).collect::<Vec<_>>()
    } else {
        api.get_entity(entity).await.unwrap_or_default()
    };

    // Apply time filters
    let mut entries: Vec<_> = entries.into_iter()
        .filter(|e| from_ts.map(|f| e.timestamp_secs.unwrap_or(0) >= f).unwrap_or(true))
        .filter(|e| to_ts.map(|t| e.timestamp_secs.unwrap_or(0) <= t).unwrap_or(true))
        .collect();
    entries.sort_by_key(|e| e.timestamp_secs.unwrap_or(0));

    let workspaces: Vec<String> = entries.iter()
        .map(|e| e.workspace_id.clone())
        .collect::<std::collections::HashSet<_>>().into_iter().collect();

    let mut conflicts = vec![];
    for ws in &workspaces {
        let mut cs = api.list_conflicts(ws).await.unwrap_or_default();
        cs.retain(|c| c.entity == entity);
        conflicts.extend(cs);
    }

    if entries.is_empty() {
        println!("{} No entries found for entity: {}", fmt::warn("⚠"), entity);
        return Ok(());
    }

    let default_path = format!("{}-audit.pdf", entity.replace('@', "_").replace('/', "_"));
    let out_path = output.unwrap_or(&default_path).to_string();

    println!("{} Generating audit trail PDF for {}…",
        fmt::dim("→"), entity.white().bold());

    // ── Build PDF ─────────────────────────────────────────────────────────────
    let (doc, page1, layer1) = PdfDocument::new(
        format!("RECALL Audit Trail — {entity}"),
        Mm(210.0), Mm(297.0), "Main",
    );

    let font      = doc.add_builtin_font(BuiltinFont::HelveticaBold)?;
    let font_reg  = doc.add_builtin_font(BuiltinFont::Helvetica)?;
    let font_mono = doc.add_builtin_font(BuiltinFont::Courier)?;

    let layer = doc.get_page(page1).get_layer(layer1);

    let mut y = Mm(280.0);
    let margin = Mm(15.0);
    let line_h  = Mm(6.0);
    let small_h = Mm(5.0);

    let advance = |y: &mut Mm, h: Mm| { *y = Mm(y.0 - h.0); };

    // Title
    layer.use_text(format!("RECALL Audit Trail"), 18.0, margin, y, &font);
    advance(&mut y, Mm(8.0));
    layer.use_text(format!("Entity: {entity}"), 11.0, margin, y, &font_reg);
    advance(&mut y, Mm(6.0));
    layer.use_text(
        format!("Generated: {}   Entries: {}   Conflicts: {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            entries.len(), conflicts.len()),
        8.0, margin, y, &font_reg
    );
    advance(&mut y, Mm(10.0));

    // Divider
    let line = Line {
        points: vec![
            (Point::new(margin, y), false),
            (Point::new(Mm(195.0), y), false),
        ],
        is_closed: false,
    };
    layer.add_line(line);
    advance(&mut y, Mm(7.0));

    // Memory Writes
    layer.use_text("MEMORY WRITES", 10.0, margin, y, &font);
    advance(&mut y, line_h);

    for e in &entries {
        if y.0 < 20.0 { break; } // avoid overflow for demo
        let ts = e.timestamp_secs
            .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
            .map(|d: chrono::DateTime<chrono::Utc>| d.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "—".into());

        layer.use_text(
            format!("{}  {}  →  {}",
                ts, e.agent_id, e.event),
            9.0, margin, y, &font_mono,
        );
        advance(&mut y, small_h);

        let val_str = match &e.data {
            serde_json::Value::Object(m) => m.get("value").map(|v| v.to_string()).unwrap_or_default(),
            v => v.to_string(),
        };
        layer.use_text(
            format!("     value: {}   tags: {}", val_str, e.tags.join(", ")),
            8.0, margin, y, &font_reg,
        );
        advance(&mut y, Mm(5.5));
        layer.use_text(
            format!("     receipt: #{}", &e.id[..e.id.len().min(32)]),
            7.0, margin, y, &font_mono,
        );
        advance(&mut y, Mm(6.5));
    }

    // Conflicts
    if !conflicts.is_empty() {
        advance(&mut y, Mm(4.0));
        layer.use_text("CONFLICTS", 10.0, margin, y, &font);
        advance(&mut y, line_h);

        for c in &conflicts {
            if y.0 < 20.0 { break; }
            layer.use_text(
                format!("⚠  {}  vs  {}   entity: {}",
                    &c.entry_a_id[..c.entry_a_id.len().min(20)],
                    &c.entry_b_id[..c.entry_b_id.len().min(20)],
                    c.entity),
                9.0, margin, y, &font_mono,
            );
            advance(&mut y, small_h);
            layer.use_text(
                format!("     resolution: {}   status: {}",
                    c.auto_resolution,
                    if c.resolution.is_empty() { "UNRESOLVED" } else { "RESOLVED" }),
                8.0, margin, y, &font_reg,
            );
            advance(&mut y, Mm(6.5));
        }
    }

    doc.save(&mut BufWriter::new(File::create(&out_path)?))?;

    println!("{} Audit trail written to {}",
        fmt::ok("✓"), out_path.green().bold());
    println!("  {}  {} writes · {} conflicts",
        fmt::label("summary:"),
        entries.len().to_string().white(),
        conflicts.len().to_string().yellow(),
    );
    println!();
    Ok(())
}
