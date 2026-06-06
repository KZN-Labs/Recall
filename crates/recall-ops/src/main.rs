use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;

mod api;
mod cmd;
mod fmt;
mod key;

use api::ApiClient;

#[derive(Parser)]
#[command(
    name    = "recall",
    about   = "RECALL — shared memory OS for AI agents",
    version = "0.1.0",
    long_about = "Inspect memory writes, conflicts, receipts, and agents across all RECALL workspaces.\nMake sure the control plane is running: cargo run -p recall-control-plane"
)]
struct Cli {
    /// Control plane HTTP endpoint
    #[arg(long, env = "RECALL_ENDPOINT", default_value = "http://localhost:8080", global = true)]
    endpoint: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Stream live memory writes across workspaces
    Logs {
        #[arg(long)] workspace: Option<String>,
        #[arg(long)] entity: Option<String>,
        /// Keep streaming (default: last 50 then exit)
        #[arg(long)] follow: bool,
    },

    /// Show all conflicts and denied writes
    Failures {
        #[arg(long)] workspace: Option<String>,
        /// Show only unresolved conflicts
        #[arg(long)] unresolved: bool,
    },

    /// Walk the receipt DAG for an entity and print the full decision trail
    Why {
        #[arg(long)] entity: String,
        #[arg(long)] workspace: Option<String>,
    },

    /// Deep dive on a single receipt, conflict, or memory entry
    Inspect {
        /// Receipt ID, conflict ID, or memory entry ID
        id: String,
    },

    /// List all agents across workspaces
    Agents {
        #[arg(long)] workspace: Option<String>,
    },

    /// Rollback a workspace to a previous point in time
    Rollback {
        #[arg(long)] workspace: String,
        /// Unix timestamp or ISO-8601 datetime (e.g. 2026-05-18T10:30:00Z)
        #[arg(long)] to: String,
    },

    /// Export full audit trail for an entity as PDF
    Export {
        #[arg(long)] entity: String,
        #[arg(long)] workspace: Option<String>,
        /// Unix timestamp — only include entries after this
        #[arg(long)] from: Option<i64>,
        /// Unix timestamp — only include entries before this
        #[arg(long)] to: Option<i64>,
        /// Output path (default: ./<entity>-audit.pdf)
        #[arg(long)] output: Option<String>,
    },

    /// Registry operations
    Registry {
        #[command(subcommand)]
        command: RegistryCommands,
    },

    /// Workspace operations
    Workspace {
        #[command(subcommand)]
        command: WorkspaceCommands,
    },

    /// Generate a new agent keypair
    Keygen {
        /// Save secret key to file
        #[arg(long)] out: Option<String>,
    },

    /// Browse recent Sui anchor commits (Merkle roots committed on-chain)
    Anchors {
        /// Number of recent anchors to show
        #[arg(long, default_value_t = 10)] limit: usize,
        /// For each anchor, fetch its Merkle root from the Walrus testnet
        /// aggregator and confirm the blob resolves (HTTP 200).
        #[arg(long)] verify: bool,
    },
}

#[derive(Subcommand)]
enum RegistryCommands {
    /// Browse published memory profiles
    List {
        #[arg(long)] category: Option<String>,
    },
    /// Show full profile details
    Inspect {
        /// Profile name or name@version
        name_version: String,
    },
    /// Import a profile into a new pre-loaded workspace
    Import {
        name_version: String,
    },
    /// Interactive publish flow
    Publish,
}

#[derive(Subcommand)]
enum WorkspaceCommands {
    /// List all workspaces
    List,
    /// Create a new workspace
    Create {
        name: String,
    },
    /// Add an agent to a workspace interactively
    AddAgent {
        #[arg(long)] workspace: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let api = ApiClient::new(&cli.endpoint);

    match cli.command {
        Commands::Logs { workspace, entity, follow } => {
            cmd::logs::run(&api,
                workspace.as_deref(),
                entity.as_deref(),
                follow,
            ).await?;
        }

        Commands::Failures { workspace, unresolved } => {
            cmd::failures::run(&api, workspace.as_deref(), unresolved).await?;
        }

        Commands::Why { entity, workspace } => {
            cmd::why::run(&api, &entity, workspace.as_deref()).await?;
        }

        Commands::Inspect { id } => {
            cmd::inspect::run(&api, &id).await?;
        }

        Commands::Agents { workspace } => {
            cmd::agents::run(&api, workspace.as_deref()).await?;
        }

        Commands::Rollback { workspace, to } => {
            cmd::rollback::run(&api, &workspace, &to).await?;
        }

        Commands::Export { entity, workspace, from, to, output } => {
            cmd::export::run(&api, &entity, workspace.as_deref(), from, to, output.as_deref()).await?;
        }

        Commands::Registry { command } => match command {
            RegistryCommands::List { category } =>
                cmd::registry::list(&api, category.as_deref()).await?,
            RegistryCommands::Inspect { name_version } =>
                cmd::registry::inspect(&api, &name_version).await?,
            RegistryCommands::Import { name_version } =>
                cmd::registry::import(&api, &name_version).await?,
            RegistryCommands::Publish =>
                cmd::registry::publish(&api).await?,
        },

        Commands::Workspace { command } => match command {
            WorkspaceCommands::List =>
                cmd::workspace::list(&api).await?,
            WorkspaceCommands::Create { name } =>
                cmd::workspace::create(&api, &name).await?,
            WorkspaceCommands::AddAgent { workspace } =>
                cmd::workspace::add_agent(&api, &workspace).await?,
        },

        Commands::Keygen { out } => {
            let kp = recall_crypto::RecallKeypair::generate();
            let secret_hex = hex::encode(kp.to_bytes());
            let pubkey_hex = hex::encode(kp.public_key().to_bytes());
            println!("{} {}", fmt::label("public key :"), pubkey_hex.cyan());
            if let Some(path) = out {
                std::fs::write(&path, &secret_hex)?;
                println!("{} {}", fmt::label("secret key :"), fmt::ok(&format!("written to {path}")));
            } else {
                println!("{} {} {}",
                    fmt::label("secret key :"),
                    secret_hex.truecolor(100,100,100),
                    fmt::dim("(store securely)"),
                );
            }
        }

        Commands::Anchors { limit, verify } => {
            cmd::anchors::run(&api, limit, verify).await?;
        }
    }

    Ok(())
}
