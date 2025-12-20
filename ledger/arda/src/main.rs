//! CLI entrypoint for the Arda orchestrator and UI shell.

use std::sync::Arc;

use clap::{Parser, Subcommand};
use ed25519_dalek::SigningKey;
use ledger_arda::{ArdaOrchestrator, ArdaUi, UiEvent, DEFAULT_SCHEMA_VERSION};
use ledger_spec::{ChannelPolicy, ChannelSpec};
use ledger_transport::{InVmQueue, Transport};
use rand_core::OsRng;
use tokio::{io::AsyncBufReadExt, io::BufReader, select};
use tracing::{error, info, Level};
use tracing_subscriber::FmtSubscriber;

/// Default domain/channel pair used by the Arda UI shell.
const DEFAULT_CHANNEL: &str = "arda.commands";

#[derive(Parser, Debug)]
#[command(author, version, about = "Arda client/orchestrator", long_about = None)]
struct Cli {
    /// Increase verbosity.
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Optional channel whitelist (repeatable). If empty, all channels are accepted.
    #[arg(short, long)]
    channel: Vec<String>,
    /// Subcommand.
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Launch interactive UI shell.
    Ui,
    /// Submit a single command payload as JSON.
    Send {
        /// Target channel.
        #[arg(short, long, default_value = DEFAULT_CHANNEL)]
        channel: String,
        /// Payload JSON string.
        payload: String,
        /// Payload type tag.
        #[arg(short = 't', long, default_value = "ea.event.v1")]
        payload_type: String,
    },
    /// Run deterministic replay validation over the current log.
    Replay,
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

    // Provision ephemeral signing identity and registry.
    let signing_key = SigningKey::generate(&mut OsRng);
    let mut registry = ledger_spec::ChannelRegistry::new();
    registry.upsert(ChannelSpec {
        name: DEFAULT_CHANNEL.into(),
        policy: ChannelPolicy {
            min_signers: 1,
            allowed_signers: vec![signing_key.verifying_key().to_bytes()],
            require_attestations: false,
            enforce_timestamp_ordering: true,
        },
    });

    let channels = if cli.channel.is_empty() {
        vec![DEFAULT_CHANNEL.to_string()]
    } else {
        cli.channel.clone()
    };

    let transport: Arc<dyn Transport> = Arc::new(InVmQueue::with_registry(registry.clone()));
    let orchestrator = ArdaOrchestrator::new(
        transport,
        registry,
        signing_key,
        channels.clone(),
        DEFAULT_SCHEMA_VERSION,
    );

    orchestrator.hydrate(256).await?;
    orchestrator.start_subscription().await?;

    match cli.command {
        Commands::Ui => launch_ui(orchestrator).await?,
        Commands::Send {
            channel,
            payload,
            payload_type,
        } => {
            let json: serde_json::Value = serde_json::from_str(&payload)?;
            let entry = orchestrator
                .submit_command(&channel, json, &payload_type, now_millis())
                .await?;
            println!("submitted {}", ArdaUi::render_entry(&entry));
        }
        Commands::Replay => {
            orchestrator.replay()?;
            println!(
                "deterministic replay OK over {} entries",
                orchestrator.log_len()
            );
        }
    }

    Ok(())
}

async fn launch_ui(orchestrator: ArdaOrchestrator) -> anyhow::Result<()> {
    let (ui, mut rx) = ArdaUi::new(orchestrator.clone());
    tokio::spawn(ui.run());
    info!("Arda UI shell ready. Type :help for commands.");

    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        select! {
            maybe_line = lines.next_line() => {
                match maybe_line? {
                    Some(line) => {
                        if line.trim().is_empty() {
                            continue;
                        }
                        if line.starts_with(":help") {
                            println!("Commands: :help, :replay, :quit, send <channel> <json>");
                            continue;
                        }
                        if line.starts_with(":quit") {
                            break;
                        }
                        if line.starts_with(":replay") {
                            match orchestrator.replay() {
                                Ok(_) => println!("replay validation OK"),
                                Err(err) => println!("replay failed: {err}"),
                            }
                            continue;
                        }
                        if let Some(rest) = line.strip_prefix("send ") {
                            if let Some((channel, payload)) = rest.split_once(' ') {
                                match serde_json::from_str::<serde_json::Value>(payload) {
                                    Ok(json) => match orchestrator
                                        .submit_command(channel, json, "ea.event.v1", now_millis())
                                        .await
                                    {
                                        Ok(entry) => println!("{}", ArdaUi::render_entry(&entry)),
                                        Err(err) => println!("submit failed: {err}"),
                                    },
                                    Err(err) => println!("invalid JSON: {err}"),
                                }
                            } else {
                                println!("usage: send <channel> <json>");
                            }
                            continue;
                        }
                        println!("unrecognized input. try :help");
                    }
                    None => break,
                }
            }
            evt = rx.recv() => {
                match evt {
                    Some(UiEvent::Ledger(entry)) => {
                        println!("{}", ArdaUi::render_entry(&entry));
                    }
                    Some(UiEvent::Status(msg)) => println!("{msg}"),
                    None => {
                        error!("ui channel closed");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
