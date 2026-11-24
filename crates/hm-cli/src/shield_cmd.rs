//! `hm shield`: write, sign and approve shields, and put them on a node.
//!
//! A shield is a rule refusing system calls or network destinations, laid
//! over sandboxes while they run (see the `hv2-shield` crate and
//! docs/SHIELDS.md). Signing happens here, offline, with a key file that
//! never leaves the operator's machine; a node is only ever sent what was
//! signed.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use hv2_shield::{
    now_secs, to_hex, verify_attestation, Action, Attestation, Directive, Event, PublicKey,
    ShieldSpec, SignedDirective, SignedShield, SigningKey,
};
use reqwest::Method;
use serde_json::{json, Value};

use crate::sandbox_vm::Api;

#[derive(Debug, Args)]
pub struct ShieldArgs {
    /// Sandbox node URL; defaults to HV2_SANDBOX_URL or http://127.0.0.1:3980
    #[arg(long, global = true)]
    pub endpoint: Option<String>,
    /// Additional PEM root certificate for the node's API
    #[arg(long, global = true)]
    pub api_ca_cert: Option<PathBuf>,
    #[command(subcommand)]
    pub command: ShieldCommand,
}

/// What a directive tells a node to do.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DirectiveAction {
    Enforce,
    Monitor,
    Remove,
}

impl From<DirectiveAction> for Action {
    fn from(action: DirectiveAction) -> Self {
        match action {
            DirectiveAction::Enforce => Action::Enforce,
            DirectiveAction::Monitor => Action::Monitor,
            DirectiveAction::Remove => Action::Remove,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum ShieldCommand {
    /// Make a signing key: its seed to FILE (readable by you alone), its
    /// public key to standard output, for a node's keyring
    Keygen {
        #[arg(long)]
        out: PathBuf,
    },
    /// Check a shield's JSON and print its digest
    Check { spec: PathBuf },
    /// Sign a shield's JSON; prints the signed shield
    Sign {
        #[arg(long)]
        key: PathBuf,
        spec: PathBuf,
    },
    /// Sign a directive about a loaded shield; prints it
    Direct {
        #[arg(long)]
        key: PathBuf,
        #[arg(value_enum)]
        action: DirectiveAction,
        /// The shield's whole SHA-256, in hex
        shield: String,
    },
    /// The node's shields, its key and its register
    List,
    /// Propose a shield for an operator to review; it does nothing until loaded
    Propose {
        spec: PathBuf,
        /// Who is proposing it, as recorded
        #[arg(long)]
        by: Option<String>,
    },
    /// Load a signed shield, in monitor mode: a file from `sign`, or a shield
    /// signed here with --key
    Load {
        /// Sign SPEC with this key first
        #[arg(long)]
        key: Option<PathBuf>,
        file: PathBuf,
    },
    /// Enforce a loaded shield (signs a directive with --key)
    Enforce {
        #[arg(long)]
        key: PathBuf,
        shield: String,
    },
    /// Return a shield to monitor mode (signs a directive with --key)
    Monitor {
        #[arg(long)]
        key: PathBuf,
        shield: String,
    },
    /// Take a shield out of force (signs a directive with --key)
    Remove {
        #[arg(long)]
        key: PathBuf,
        shield: String,
    },
    /// Send a directive signed elsewhere
    Apply { file: PathBuf },
    /// Withdraw a proposal
    Withdraw { shield: String },
    /// What shields matched: refused, or recorded while monitored
    Hits,
    /// Have the node read its keyring file again
    ReloadKeyring,
    /// Ask the node to attest to what has been in force, over a fresh nonce,
    /// and check the answer: its signature, the nonce, and the log's replay
    Attest {
        /// The node's attestation public key (hex), as `list` gave it when the
        /// node was enrolled; without it the key the node names is used,
        /// which proves only that the log is consistent
        #[arg(long)]
        node_key: Option<String>,
    },
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))
}

fn key_from(path: &Path) -> Result<SigningKey> {
    SigningKey::from_seed_hex(read(path)?.trim())
        .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

fn write_secret(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("creating {} (it must not exist yet)", path.display()))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

fn directive(key: &Path, action: Action, shield: &str) -> Result<SignedDirective> {
    let key = key_from(key)?;
    SignedDirective::sign(
        &Directive {
            action,
            shield: shield.to_ascii_lowercase(),
            issued_at: now_secs(),
        },
        &key,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn api(args: &ShieldArgs) -> Result<Api> {
    let endpoint = args
        .endpoint
        .clone()
        .or_else(|| std::env::var("HV2_SANDBOX_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:3980".into());
    crate::sandbox_vm::api_for(&endpoint, 60, args.api_ca_cert.as_deref())
}

/// Run `hm shield`, printing JSON.
pub async fn run(args: ShieldArgs) -> Result<i32> {
    let value = match &args.command {
        ShieldCommand::Keygen { out } => {
            let key = SigningKey::generate().map_err(|e| anyhow::anyhow!("{e}"))?;
            write_secret(out, &key.seed_hex())?;
            json!({
                "keyId": key.public_key().id(),
                "publicKey": key.public_key().to_hex(),
                "seed": out.display().to_string(),
            })
        }
        ShieldCommand::Check { spec } => {
            let text = read(spec)?;
            let parsed = ShieldSpec::parse(&text).map_err(|e| anyhow::anyhow!("{e}"))?;
            json!({
                "name": parsed.name,
                "digest": to_hex(&hv2_shield::sha256(text.as_bytes())),
                "expiresAt": parsed.expires_at,
            })
        }
        ShieldCommand::Sign { key, spec } => {
            let signed = SignedShield::sign(&read(spec)?, &key_from(key)?)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            serde_json::to_value(signed)?
        }
        ShieldCommand::Direct {
            key,
            action,
            shield,
        } => serde_json::to_value(directive(key, (*action).into(), shield)?)?,
        ShieldCommand::List => api(&args)?.request(Method::GET, &["shields"], None).await?,
        ShieldCommand::Propose { spec, by } => {
            api(&args)?
                .request(
                    Method::POST,
                    &["shields", "proposals"],
                    Some(json!({ "spec": read(spec)?, "proposedBy": by })),
                )
                .await?
        }
        ShieldCommand::Load { key, file } => {
            let text = read(file)?;
            let signed: SignedShield = match key {
                Some(key) => SignedShield::sign(&text, &key_from(key)?)
                    .map_err(|e| anyhow::anyhow!("{e}"))?,
                None => serde_json::from_str(&text)
                    .with_context(|| format!("{} is not a signed shield", file.display()))?,
            };
            api(&args)?
                .request(
                    Method::POST,
                    &["shields"],
                    Some(serde_json::to_value(signed)?),
                )
                .await?
        }
        ShieldCommand::Enforce { key, shield }
        | ShieldCommand::Monitor { key, shield }
        | ShieldCommand::Remove { key, shield } => {
            let action = match &args.command {
                ShieldCommand::Enforce { .. } => Action::Enforce,
                ShieldCommand::Monitor { .. } => Action::Monitor,
                _ => Action::Remove,
            };
            let signed = directive(key, action, shield)?;
            api(&args)?
                .request(
                    Method::POST,
                    &["shields", "directives"],
                    Some(serde_json::to_value(signed)?),
                )
                .await?
        }
        ShieldCommand::Apply { file } => {
            let signed: SignedDirective = serde_json::from_str(&read(file)?)
                .with_context(|| format!("{} is not a signed directive", file.display()))?;
            api(&args)?
                .request(
                    Method::POST,
                    &["shields", "directives"],
                    Some(serde_json::to_value(signed)?),
                )
                .await?
        }
        ShieldCommand::Withdraw { shield } => {
            api(&args)?
                .request(Method::DELETE, &["shields", "proposals", shield], None)
                .await?;
            json!({ "withdrawn": shield })
        }
        ShieldCommand::Hits => {
            api(&args)?
                .request(Method::GET, &["shields", "hits"], None)
                .await?
        }
        ShieldCommand::ReloadKeyring => {
            api(&args)?
                .request(Method::POST, &["shields", "keyring"], None)
                .await?
        }
        ShieldCommand::Attest { node_key } => attest(&api(&args)?, node_key.as_deref()).await?,
    };
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(0)
}

/// Ask for an attestation over a fresh nonce, and check it.
async fn attest(api: &Api, pinned: Option<&str>) -> Result<Value> {
    let nonce = hv2_shield::fresh_nonce().map_err(|e| anyhow::anyhow!("{e}"))?;
    let attestation: Attestation = serde_json::from_value(
        api.request(
            Method::POST,
            &["shields", "attestation"],
            Some(json!({ "nonce": to_hex(&nonce) })),
        )
        .await?,
    )
    .context("the node's attestation does not parse")?;
    let listed = api.request(Method::GET, &["shields"], None).await?;
    let key_hex = match pinned {
        Some(key) => key.to_string(),
        None => listed["nodePublicKey"]
            .as_str()
            .context("the node names no attestation key")?
            .to_string(),
    };
    let node_key = PublicKey::from_hex(&key_hex).map_err(|e| anyhow::anyhow!("{e}"))?;
    let log = api
        .request(Method::GET, &["shields", "events"], None)
        .await?;
    let events: Vec<Event> =
        serde_json::from_value(log["events"].clone()).context("the node's events do not parse")?;
    let verified = verify_attestation(&attestation, &node_key, &nonce, &events)
        .map_err(|e| anyhow::anyhow!("the attestation does not check out: {e}"))?;
    Ok(json!({
        "verified": true,
        "nodeKey": node_key.id(),
        "keyPinned": pinned.is_some(),
        "register": attestation.register,
        "at": attestation.at,
        "events": verified,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_file_is_made_once_and_signs_what_a_node_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("operator.key");
        let key = SigningKey::generate().unwrap();
        write_secret(&out, &key.seed_hex()).unwrap();
        assert!(write_secret(&out, "x").is_err(), "never overwritten");
        let read_back = key_from(&out).unwrap();
        assert_eq!(read_back.public_key(), key.public_key());

        let spec = r#"{"name":"no-ptrace","syscalls":[{"syscall":"ptrace","action":{"kind":"errno","errno":1}}],"expiresAt":4102444800}"#;
        let signed = SignedShield::sign(spec, &read_back).unwrap();
        let mut keyring = hv2_shield::Keyring::default();
        keyring.trust(key.public_key().clone());
        assert!(signed.verify(&keyring).is_ok());

        let digest = to_hex(&signed.digest());
        let signed = directive(&out, Action::Enforce, &digest.to_ascii_uppercase()).unwrap();
        assert!(signed.verify(&keyring, now_secs()).is_ok());
        assert!(hv2_shield::from_hex(&digest).is_some());
    }
}
