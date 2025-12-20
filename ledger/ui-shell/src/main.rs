//! Cross-platform ledger UI shell: ledger-backed views, command submission, and Merkle receipts.
//!
//! The shell prioritizes verifiability over raw execution. Every action is recorded as a
//! ledgered envelope, Merkle receipts are emitted for each entry, and printable receipts are
//! hash-stamped for offline verification. The UI remains minimal yet is designed for production
//! operability across macOS, Linux, and Windows terminals.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use blake3::Hasher;
use clap::{Parser, Subcommand, ValueEnum};
use ledger_core::{AppendLog, MerkleReceipt};
use ledger_spec::{self, envelope_hash, ChannelPolicy, ChannelRegistry, Envelope, EnvelopeBody};
use serde::Serialize;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// UI rendering mode.
#[derive(Debug, Clone, Copy, ValueEnum)]
enum UiMode {
    /// Minimal output (default).
    Minimal,
    /// Verbose with hashes and receipt paths.
    Verbose,
}

/// Printable receipt with a hash stamp for verification.
#[derive(Debug, Serialize)]
struct PrintableReceipt {
    /// Hex-encoded receipt payload (serialized MerkleReceipt).
    receipt_hex: String,
    /// Human description of the action.
    description: String,
    /// Log index.
    index: usize,
    /// Merkle root.
    root_hex: String,
    /// Blake3 hash over the rendered receipt for offline verification.
    stamp_hex: String,
}

/// Ledger UI shell CLI.
#[derive(Parser, Debug)]
#[command(author, version, about = "Ledger UI shell: proof-first interactions", long_about = None)]
struct Cli {
    /// Increase output verbosity.
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// UI rendering mode.
    #[arg(short = 'm', long, value_enum, default_value = "minimal")]
    mode: UiMode,

    /// Subcommand to execute.
    #[command(subcommand)]
    command: Commands,
}

/// Subcommands for the UI shell.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Submit a JSON command payload. Payload is ledgered; no direct execution occurs.
    Submit {
        /// Path to JSON file representing the command.
        #[arg(short, long)]
        file: PathBuf,
    },
    /// Show ledger entries with Merkle proofs.
    View {
        /// Starting offset.
        #[arg(short, long, default_value = "0")]
        offset: usize,
        /// Max entries to display.
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// Print a receipt for an entry.
    Receipt {
        /// Log index to print a receipt for.
        index: usize,
        /// Optional output path; defaults to a temp file.
        #[arg(short, long)]
        out: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let level = match cli.verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        Commands::Submit { file } => submit(file).await?,
        Commands::View { offset, limit } => view(offset, limit, cli.mode).await?,
        Commands::Receipt { index, out } => receipt(index, out).await?,
    }

    Ok(())
}

async fn submit(path: PathBuf) -> Result<()> {
    let payload = read_json(&path)
        .await
        .with_context(|| format!("parsing JSON from {}", path.display()))?;

    let registry = default_registry();
    let ledger_path = ledger_store_path()?;
    let mut log = load_log(&ledger_path, &registry)?;

    // Ledger-only capture: no direct execution. We bind prev to the last entry to preserve hash
    // chaining, and we permit zero-signature commands via the relaxed registry policy.
    let prev_hash = last_hash(&log);
    let body = EnvelopeBody {
        payload: payload.clone(),
        payload_type: Some("ui-command".into()),
    };
    let body_hash = ledger_spec::hash_body(&body);
    let env = Envelope {
        header: ledger_spec::EnvelopeHeader {
            channel: "ui_commands".into(),
            version: 1,
            prev: prev_hash,
            body_hash,
            timestamp: current_ts(),
        },
        body,
        signatures: Vec::new(),
        attestations: Vec::new(),
    };

    log.append(env.clone(), &registry)?;
    persist_log(&ledger_path, &log)?;

    let idx = log.len().saturating_sub(1);
    let receipt = log
        .receipt_for(idx)
        .context("generating Merkle receipt for command")?;
    print_receipt(&receipt, None)?;

    info!(index = idx, "submitted command to ledger");
    Ok(())
}

async fn view(offset: usize, limit: usize, mode: UiMode) -> Result<()> {
    let registry = default_registry();
    let ledger_path = ledger_store_path()?;
    let log = load_log(&ledger_path, &registry)?;
    let total = log.len();
    let entries = log.read(offset, limit);
    for (idx, env) in entries.into_iter().enumerate() {
        let log_index = offset + idx;
        let summary = env.body.payload.to_string();
        let receipt = log.receipt_for(log_index);
        match mode {
            UiMode::Minimal => {
                println!(
                    "[{}] channel={} ts={} payload={} ({} total entries)",
                    log_index, env.header.channel, env.header.timestamp, summary, total
                );
            }
            UiMode::Verbose => {
                let receipt_hex = receipt
                    .as_ref()
                    .map(|r| to_hex(&serde_json::to_vec(r).unwrap()))
                    .unwrap_or_else(|| "n/a".into());
                println!(
                    "[{}] channel={} ts={} payload={} prev={} receipt_hex={} ({} total entries)",
                    log_index,
                    env.header.channel,
                    env.header.timestamp,
                    summary,
                    env.header
                        .prev
                        .map(|p| to_hex(&p))
                        .unwrap_or_else(|| "genesis".into()),
                    receipt_hex,
                    total
                );
            }
        }
    }
    Ok(())
}

async fn receipt(index: usize, out: Option<PathBuf>) -> Result<()> {
    let registry = default_registry();
    let ledger_path = ledger_store_path()?;
    let log = load_log(&ledger_path, &registry)?;
    let receipt = log
        .receipt_for(index)
        .with_context(|| format!("no receipt available at index {index}"))?;
    print_receipt(&receipt, out)?;
    Ok(())
}

fn ledger_store_path() -> Result<PathBuf> {
    if let Ok(custom) = std::env::var("EA_LEDGER_PATH") {
        let path = PathBuf::from(custom);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent directory {}", parent.display()))?;
        }
        return Ok(path);
    }

    let base = match std::env::var("APPDATA")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
    {
        Ok(dir) => dir,
        Err(_) => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    let dir = base.join("ea_ledger");
    std::fs::create_dir_all(&dir).context("creating ledger data directory")?;
    Ok(dir.join("ui-ledger.json"))
}

fn default_registry() -> ChannelRegistry {
    let mut registry = ChannelRegistry::new();
    registry.upsert(ledger_spec::ChannelSpec {
        name: "ui_commands".into(),
        policy: ChannelPolicy {
            min_signers: 0,
            allowed_signers: vec![],
            require_attestations: false,
            enforce_timestamp_ordering: true,
        },
    });
    registry
}

fn load_log(path: &Path, registry: &ChannelRegistry) -> Result<AppendLog> {
    let mut log = AppendLog::new();
    if path.exists() {
        let data =
            std::fs::read(path).with_context(|| format!("reading ledger at {}", path.display()))?;
        if !data.is_empty() {
            let entries: Vec<Envelope> =
                serde_json::from_slice(&data).context("parsing stored ledger entries")?;
            for env in entries {
                log.append(env, registry)?;
            }
        }
    }
    Ok(log)
}

fn persist_log(path: &Path, log: &AppendLog) -> Result<()> {
    let entries = log.read(0, log.len());
    let encoded = serde_json::to_vec_pretty(&entries).context("serializing ledger entries")?;
    std::fs::write(path, encoded).with_context(|| format!("writing ledger to {}", path.display()))
}

fn print_receipt(receipt: &MerkleReceipt, out: Option<PathBuf>) -> Result<()> {
    let rendered_receipt = serde_json::to_vec(receipt).context("serializing receipt")?;
    let stamp = blake3_hex(&rendered_receipt);
    let printable = PrintableReceipt {
        receipt_hex: to_hex(&rendered_receipt),
        description: format!(
            "Merkle receipt for log index {} with {} leaves",
            receipt.index, receipt.leaf_count
        ),
        index: receipt.index,
        root_hex: to_hex(&receipt.root),
        stamp_hex: stamp.clone(),
    };

    let rendered = serde_json::to_string_pretty(&printable)?;
    match out {
        Some(path) => {
            let mut file = File::create(&path)
                .with_context(|| format!("creating receipt file at {}", path.display()))?;
            file.write_all(rendered.as_bytes())?;
            println!("Receipt written to {} (stamp={})", path.display(), stamp);
        }
        None => {
            let path = temp_receipt_path();
            let mut file = File::create(&path)
                .with_context(|| format!("creating temp receipt file at {}", path.display()))?;
            file.write_all(rendered.as_bytes())?;
            println!("Receipt written to {} (stamp={})", path.display(), stamp);
        }
    }
    Ok(())
}

fn temp_receipt_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    path.push(format!("ledger-receipt-{nanos}.json"));
    path
}

fn blake3_hex(input: &[u8]) -> String {
    let mut h = Hasher::new();
    h.update(input);
    h.finalize().to_hex().to_string()
}

fn last_hash(log: &AppendLog) -> Option<[u8; 32]> {
    let len = log.len();
    if len == 0 {
        None
    } else {
        log.read(len - 1, 1).first().map(envelope_hash)
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn current_ts() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path() -> PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        dir.push(format!("ledger-ui-shell-test-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("ledger.json")
    }

    #[tokio::test]
    async fn append_and_persist_roundtrip() {
        let path = temp_path();
        let registry = default_registry();
        let mut log = load_log(&path, &registry).unwrap();
        assert_eq!(log.len(), 0);

        let body = serde_json::json!({ "cmd": "test" });
        let env = Envelope {
            header: ledger_spec::EnvelopeHeader {
                channel: "ui_commands".into(),
                version: 1,
                prev: None,
                body_hash: ledger_spec::hash_body(&EnvelopeBody {
                    payload: body.clone(),
                    payload_type: Some("ui-command".into()),
                }),
                timestamp: 1,
            },
            body: EnvelopeBody {
                payload: body,
                payload_type: Some("ui-command".into()),
            },
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        log.append(env, &registry).unwrap();
        persist_log(&path, &log).unwrap();

        let reloaded = load_log(&path, &registry).unwrap();
        assert_eq!(reloaded.len(), 1);
        assert!(reloaded.receipt_for(0).unwrap().verify());

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn blake3_stamp_matches() {
        let msg = b"hello";
        let stamp = blake3_hex(msg);
        let expected = blake3::hash(msg).to_hex().to_string();
        assert_eq!(stamp, expected);
    }
}
