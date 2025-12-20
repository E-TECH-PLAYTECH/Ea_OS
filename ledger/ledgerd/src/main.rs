//! `ledgerd` CLI/daemon for append/read/subscribe with policy filters and audit checkpoints.

use clap::{Parser, Subcommand};
use ledger_core::{AppendLog, CheckpointWriter};
use ledger_transport::{InVmQueue, Transport};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Ledgerd command line.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Increase output verbosity.
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Subcommand.
    #[command(subcommand)]
    command: Commands,
}

/// Commands for ledgerd.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Run daemon.
    Daemon {
        /// Checkpoint interval.
        #[arg(short, long, default_value = "10")]
        checkpoint: usize,
    },
    /// Append an envelope from JSON.
    Append {
        /// JSON file containing the envelope.
        #[arg(short, long)]
        file: String,
    },
    /// Read envelopes.
    Read {
        /// Start offset.
        #[arg(short, long, default_value = "0")]
        offset: usize,
        /// Number of entries.
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let level = match cli.verbose {
        0 => Level::INFO,
        1 => Level::DEBUG,
        _ => Level::TRACE,
    };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        Commands::Daemon { checkpoint } => daemon(checkpoint).await?,
        Commands::Append { file } => append_from_file(file).await?,
        Commands::Read { offset, limit } => read_entries(offset, limit).await?,
    }
    Ok(())
}

async fn daemon(checkpoint_interval: usize) -> anyhow::Result<()> {
    let queue = InVmQueue::new();
    let mut writer = CheckpointWriter::new();
    let mut rx = queue.subscribe().await?;
    info!("ledgerd daemon started");
    loop {
        let env = rx.recv().await?;
        info!(
            "received envelope channel={} ts={}",
            env.header.channel, env.header.timestamp
        );
        if let Some(cp) = writer.maybe_checkpoint(&queue.log, checkpoint_interval) {
            info!("checkpoint length={} root={:x?}", cp.length, cp.root);
        }
    }
}

async fn append_from_file(path: String) -> anyhow::Result<()> {
    let data = tokio::fs::read(&path).await?;
    let mut env: ledger_spec::Envelope = serde_json::from_slice(&data)?;
    // For demo, auto-sign with ephemeral key if no signatures.
    if env.signatures.is_empty() {
        let sk = ed25519_dalek::SigningKey::generate(&mut rand_core::OsRng);
        ledger_core::signing::sign_envelope(&mut env, &sk);
    }
    let queue = InVmQueue::new();
    queue.append(env.clone()).await?;
    info!("appended envelope ts={}", env.header.timestamp);
    Ok(())
}

async fn read_entries(offset: usize, limit: usize) -> anyhow::Result<()> {
    let queue = InVmQueue::new();
    let items = queue.read(offset, limit).await?;
    for env in items {
        println!(
            "channel={} ts={} payload={}",
            env.header.channel, env.header.timestamp, env.body.payload
        );
    }
    Ok(())
}
