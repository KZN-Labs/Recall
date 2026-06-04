//! Agent keypair persistence for the `recall` CLI.
//!
//! Default location: `~/.recall/agent.key`
//!
//! Stored as raw 32-byte Ed25519 private key. The public key is derived on load.
//! Passport ID is the SHA-256 of the hex-encoded public key, matching the
//! convention used by the Python and TypeScript SDKs so the same agent
//! identity is consistent across surfaces.

use anyhow::{Context, Result};
use recall_crypto::{sha256_hex, RecallKeypair};
use std::fs;
use std::path::PathBuf;

const KEY_DIR_NAME:  &str = ".recall";
const KEY_FILE_NAME: &str = "agent.key";

pub struct AgentIdentity {
    pub keypair:        RecallKeypair,
    pub passport_id:    String,
    pub public_key_hex: String,
    pub generated:      bool,
}

pub fn default_key_path() -> Result<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("could not resolve home directory (HOME/USERPROFILE not set)")?;
    Ok(PathBuf::from(home).join(KEY_DIR_NAME).join(KEY_FILE_NAME))
}

/// Load the agent's keypair from `path`, or generate a new one and save it
/// if the file does not yet exist.
pub fn load_or_create(path: Option<PathBuf>) -> Result<AgentIdentity> {
    let path = match path {
        Some(p) => p,
        None    => default_key_path()?,
    };

    if path.exists() {
        let raw = fs::read(&path)
            .with_context(|| format!("read keyfile {}", path.display()))?;
        let key_bytes: [u8; 32] = raw
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("keyfile {} is not 32 bytes", path.display()))?;
        let keypair = RecallKeypair::from_bytes(&key_bytes);
        let pub_hex = hex::encode(keypair.public_key().to_bytes());
        let passport_id = sha256_hex(pub_hex.as_bytes());
        Ok(AgentIdentity {
            keypair,
            passport_id,
            public_key_hex: pub_hex,
            generated: false,
        })
    } else {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create key dir {}", parent.display()))?;
        }
        let keypair = RecallKeypair::generate();
        let key_bytes = keypair.to_bytes();
        fs::write(&path, key_bytes)
            .with_context(|| format!("write keyfile {}", path.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }

        let pub_hex = hex::encode(keypair.public_key().to_bytes());
        let passport_id = sha256_hex(pub_hex.as_bytes());
        Ok(AgentIdentity {
            keypair,
            passport_id,
            public_key_hex: pub_hex,
            generated: true,
        })
    }
}
