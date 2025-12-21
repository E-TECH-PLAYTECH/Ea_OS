//! `ledgerd` CLI/daemon for append/read/subscribe with policy filters and audit checkpoints.

use clap::{Args, Parser, Subcommand, ValueEnum};
use ledger_core::{AppendLog, CheckpointWriter};
use ledger_spec::{ChannelRegistry, ChannelSpec};
use ledger_transport::{
    bind_transport, AdapterCapability, AdapterKind, CapabilityAdvertisement, Transport,
    TransportConfig, TransportDomain,
};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

/// Ledgerd command line.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Increase output verbosity.
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Transport configuration flags.
    #[command(flatten)]
    transport: TransportCli,
    /// Channel registry definition.
    #[arg(
        long,
        env = "LEDGER_REGISTRY",
        value_name = "FILE",
        help = "Path to a JSON-encoded ChannelSpec list"
    )]
    registry: String,
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

/// Transport selection flags.
#[derive(Args, Debug, Clone)]
struct TransportCli {
    /// Transport kind.
    #[arg(
        long,
        value_enum,
        default_value_t = TransportKind::Unix,
        env = "LEDGER_TRANSPORT"
    )]
    transport: TransportKind,
    /// Unix socket path for IPC transport.
    #[arg(
        long,
        env = "LEDGER_UNIX_PATH",
        default_value = "/tmp/ledgerd.sock",
        value_name = "PATH",
        help = "Filesystem path for the Unix domain socket transport"
    )]
    unix_path: String,
    /// QUIC/gRPC endpoint for remote daemon transport.
    #[arg(
        long,
        env = "LEDGER_QUIC_ENDPOINT",
        value_name = "ENDPOINT",
        help = "Authority/endpoint for QUIC transport (e.g. https://ledgerd.example.com)"
    )]
    quic_endpoint: Option<String>,
}

/// Supported transports exposed via CLI.
#[derive(ValueEnum, Clone, Debug)]
enum TransportKind {
    Loopback,
    Unix,
    Quic,
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

    let registry = load_registry(&cli.registry).await?;
    let transport_config = build_transport_config(&cli.transport)?;
    let transport = bind_transport(registry.clone(), transport_config.clone()).await?;

    match cli.command {
        Commands::Daemon { checkpoint } => daemon(checkpoint, transport, registry).await?,
        Commands::Append { file } => append_from_file(file, transport).await?,
        Commands::Read { offset, limit } => read_entries(offset, limit, transport).await?,
    }
    Ok(())
}

async fn daemon(
    checkpoint_interval: usize,
    transport: std::sync::Arc<dyn Transport>,
    registry: ChannelRegistry,
) -> anyhow::Result<()> {
    let mut writer = CheckpointWriter::new();
    let mut rx = transport.subscribe().await?;
    let log = AppendLog::new();
    info!("ledgerd daemon started");
    loop {
        let env = rx.recv().await?;
        log.append(env.clone(), &registry)?;
        info!(
            "received envelope channel={} ts={}",
            env.header.channel, env.header.timestamp
        );
        if let Some(cp) = writer.maybe_checkpoint(&log, checkpoint_interval) {
            info!("checkpoint length={} root={:x?}", cp.length, cp.root);
        }
    }
}

async fn append_from_file(
    path: String,
    transport: std::sync::Arc<dyn Transport>,
) -> anyhow::Result<()> {
    let data = tokio::fs::read(&path).await?;
    let mut env: ledger_spec::Envelope = serde_json::from_slice(&data)?;
    // For demo, auto-sign with ephemeral key if no signatures.
    if env.signatures.is_empty() {
        let sk = ed25519_dalek::SigningKey::generate(&mut rand_core::OsRng);
        ledger_core::signing::sign_envelope(&mut env, &sk);
    }
    transport.append(env.clone()).await?;
    info!("appended envelope ts={}", env.header.timestamp);
    Ok(())
}

async fn read_entries(
    offset: usize,
    limit: usize,
    transport: std::sync::Arc<dyn Transport>,
) -> anyhow::Result<()> {
    let items = transport.read(offset, limit).await?;
    for env in items {
        println!(
            "channel={} ts={} payload={}",
            env.header.channel, env.header.timestamp, env.body.payload
        );
    }
    Ok(())
}

async fn load_registry(path: &str) -> anyhow::Result<ChannelRegistry> {
    let data = tokio::fs::read(path).await?;
    let specs: Vec<ChannelSpec> = serde_json::from_slice(&data)?;
    let mut registry = ChannelRegistry::new();
    for spec in specs {
        registry.upsert(spec);
    }
    Ok(registry)
}

fn build_transport_config(cli: &TransportCli) -> anyhow::Result<TransportConfig> {
    match cli.transport {
        TransportKind::Loopback => Ok(TransportConfig::loopback(TransportDomain::Ledger)),
        TransportKind::Unix => {
            let path = cli.unix_path.clone();
            let selected = AdapterCapability {
                adapter: AdapterKind::UnixIpc { path: path.clone() },
                features: vec![],
                attestation: None,
            };
            let advertisement = CapabilityAdvertisement {
                domain: TransportDomain::Ledger,
                supported_versions: vec!["1.0.x".into()],
                max_message_bytes: 1_048_576,
                adapters: vec![selected.clone()],
            };
            Ok(TransportConfig {
                advertisement,
                selected,
            })
        }
        TransportKind::Quic => {
            let endpoint = cli
                .quic_endpoint
                .clone()
                .ok_or_else(|| anyhow::anyhow!("--quic-endpoint is required for quic transport"))?;
            let selected = AdapterCapability {
                adapter: AdapterKind::QuicGrpc {
                    endpoint: endpoint.clone(),
                    alpn: None,
                },
                features: vec![],
                attestation: None,
            };
            let advertisement = CapabilityAdvertisement {
                domain: TransportDomain::Ledger,
                supported_versions: vec!["1.0.x".into()],
                max_message_bytes: 1_048_576,
                adapters: vec![selected.clone()],
            };
            Ok(TransportConfig {
                advertisement,
                selected,
            })
        }
    }
}
