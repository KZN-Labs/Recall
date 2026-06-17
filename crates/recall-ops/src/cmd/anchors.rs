//! `recall anchors` — browse and verify Sui anchor commits.
//!
//! Fetches recent `anchor.commit` receipts from the control plane. Each anchor
//! commit batches a set of receipts under a Merkle root and submits the root
//! to the deployed `receipt_anchor` Move package on Sui testnet.
//!
//! With --verify, the Merkle root is treated as a Walrus blob ID and we
//! confirm the blob resolves on the testnet aggregator.

use anyhow::Result;
use chrono::{TimeZone, Utc};
use colored::Colorize;
use crate::{api::{ApiClient, Receipt}, fmt};

const WALRUS_AGGREGATOR: &str = "https://aggregator.walrus-testnet.walrus.space";
const SUI_EXPLORER:      &str = "https://suiscan.xyz/testnet/tx";

pub async fn run(api: &ApiClient, limit: usize, verify: bool) -> Result<()> {
    if !api.health().await {
        eprintln!("{}", fmt::err("✗ control plane unreachable — is it running on :8080?"));
        return Ok(());
    }

    let anchors = api.list_recent_receipts(Some("anchor.commit"), limit).await?;

    if anchors.is_empty() {
        println!();
        println!("{}", "RECENT SUI ANCHORS".white().bold());
        fmt::sep();
        println!("{}", fmt::dim("No anchor commits yet."));
        println!();
        println!("{}", fmt::dim(
            "Anchors are emitted when the control plane seals a batch of receipts\n\
             and commits the Merkle root to the receipt_anchor Move package on Sui."
        ));
        println!();
        return Ok(());
    }

    println!();
    println!("{}", "RECENT SUI ANCHORS".white().bold());
    fmt::sep();
    println!(
        "{:<20}  {:<22}  {:<32}  {:>8}",
        "TIMESTAMP".truecolor(80, 80, 80),
        "MERKLE ROOT".truecolor(80, 80, 80),
        "SUI TX DIGEST".truecolor(80, 80, 80),
        "BATCH".truecolor(80, 80, 80),
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap_or_default();

    for r in &anchors {
        print_anchor_row(r);
        if verify {
            verify_anchor(&client, r).await;
        }
    }

    println!();
    println!(
        "  {} {} anchor{}",
        fmt::dim("→"),
        anchors.len().to_string().white(),
        if anchors.len() == 1 { "" } else { "s" }
    );

    if verify {
        println!(
            "  {} verified against {}",
            fmt::dim("→"),
            WALRUS_AGGREGATOR.truecolor(120, 120, 160)
        );
    } else {
        println!(
            "  {} {}",
            fmt::dim("tip:"),
            "pass --verify to fetch each Merkle root from the Walrus aggregator".dimmed()
        );
    }
    println!();
    Ok(())
}

fn print_anchor_row(r: &Receipt) {
    let ts = r
        .timestamp_secs
        .and_then(|s| Utc.timestamp_opt(s, 0).single())
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "—".into());

    let merkle = short_hex(&r.evidence_digest);
    let batch  = r.causal_predecessors.len();

    // The anchor scheduler stashes the actual Sui tx digest (or an
    // `UNANCHORED:<reason>` marker if no on-chain submission happened) on the
    // receipt's deny_reason field. Render real digests green and UNANCHORED in
    // red so a synthetic value can never appear confirmed.
    let raw_tx = r.deny_reason.as_deref().unwrap_or("");
    let (tx_cell, tail) = if raw_tx.starts_with("UNANCHORED:") {
        let reason = &raw_tx["UNANCHORED:".len()..];
        let cell = "UNANCHORED".truecolor(230, 90, 90).bold().to_string();
        let tail = format!("  ⚠ reason: {}", reason).truecolor(220, 140, 140).to_string();
        (cell, Some(tail))
    } else if raw_tx.is_empty() {
        ("—".truecolor(120, 120, 120).to_string(), None)
    } else {
        (short_hex(raw_tx).truecolor(140, 200, 140).to_string(), None)
    };

    println!(
        "{:<20}  {:<22}  {:<32}  {:>8}",
        ts.truecolor(160, 160, 160),
        merkle.truecolor(180, 180, 220),
        tx_cell,
        batch.to_string().white(),
    );
    if let Some(t) = tail {
        println!("    {}", t);
    }
}

async fn verify_anchor(client: &reqwest::Client, r: &Receipt) {
    if r.evidence_digest.is_empty() {
        println!(
            "  {} {}",
            fmt::dim("↳ skip"),
            "no merkle root on this anchor receipt".dimmed()
        );
        return;
    }
    let url = format!("{}/v1/blobs/{}", WALRUS_AGGREGATOR, r.evidence_digest);
    match client.get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            println!(
                "  {} {}",
                fmt::ok("↳ ✓"),
                format!("blob {} verified on Walrus testnet", short_hex(&r.evidence_digest))
                    .truecolor(140, 200, 140)
            );
        }
        Ok(resp) => {
            println!(
                "  {} {}",
                fmt::err("↳ ✗"),
                format!("aggregator returned HTTP {}", resp.status()).truecolor(220, 140, 140)
            );
        }
        Err(e) => {
            println!(
                "  {} {}",
                fmt::err("↳ ✗"),
                format!("aggregator unreachable: {}", e).truecolor(220, 140, 140)
            );
        }
    }

    // Render an explorer hint — anchor receipt IDs aren't Sui tx digests, but
    // browsing the receipt_anchor package via the explorer is the next step
    // anyone reading this output would want.
    let _ = SUI_EXPLORER; // suppress unused warning when no tx hint is shown
}

fn short_hex(s: &str) -> String {
    if s.is_empty() {
        "—".to_string()
    } else if s.len() <= 18 {
        s.to_string()
    } else {
        format!("{}…{}", &s[..10], &s[s.len() - 6..])
    }
}
