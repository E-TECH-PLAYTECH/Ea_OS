//! `ledgerd` CLI/daemon for append/read/subscribe with policy filters and audit checkpoints.

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use clap::{Args, Parser, Subcommand, ValueEnum};
use ledger_core::{AppendLog, CheckpointWriter};
use ledger_spec::{ChannelRegistry, ChannelSpec};
use ledger_transport::{
    bind_transport, AdapterCapability, AdapterKind, CapabilityAdvertisement, Transport,
    TransportConfig, TransportDomain,
};
use prometheus::Encoder;
use serde::Serialize;
use tracing::{info, Level};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

#[derive(Clone)]
struct DaemonMetrics {
    registry: prometheus::Registry,
    appends_total: prometheus::IntCounterVec,
    append_errors_total: prometheus::IntCounterVec,
    append_latency_ms: prometheus::HistogramVec,
    read_latency_ms: prometheus::HistogramVec,
    backlog_gauge: prometheus::IntGauge,
    disk_usage_bytes: prometheus::IntGauge,
    attestation_status: prometheus::IntGauge,
}

impl DaemonMetrics {
    fn new(attestation_configured: bool) -> Self {
        let registry = prometheus::Registry::new();
        let appends_total = prometheus::IntCounterVec::new(
            prometheus::Opts::new("ledgerd_appends_total", "Total envelopes appended"),
            &["channel"],
        )
        .unwrap();
        let append_errors_total = prometheus::IntCounterVec::new(
            prometheus::Opts::new("ledgerd_append_errors_total", "Append errors by channel"),
            &["channel"],
        )
        .unwrap();
        let append_latency_ms = prometheus::HistogramVec::new(
            prometheus::HistogramOpts::new(
                "ledgerd_append_latency_ms",
                "Append latency in milliseconds",
            ),
            &["channel"],
        )
        .unwrap();
        let read_latency_ms = prometheus::HistogramVec::new(
            prometheus::HistogramOpts::new(
                "ledgerd_read_latency_ms",
                "Read latency in milliseconds",
            ),
            &["channel"],
        )
        .unwrap();
        let backlog_gauge =
            prometheus::IntGauge::new("ledgerd_backlog", "Pending envelopes in the receive buffer")
                .unwrap();
        let disk_usage_bytes = prometheus::IntGauge::new(
            "ledgerd_disk_usage_bytes",
            "Estimated disk usage of the ledger log",
        )
        .unwrap();
        let attestation_status = prometheus::IntGauge::new(
            "ledgerd_attestation_configured",
            "Whether attestation is configured (1) or not (0)",
        )
        .unwrap();

        registry
            .register(Box::new(appends_total.clone()))
            .expect("register appends_total");
        registry
            .register(Box::new(append_errors_total.clone()))
            .expect("register append_errors_total");
        registry
            .register(Box::new(append_latency_ms.clone()))
            .expect("register append_latency_ms");
        registry
            .register(Box::new(read_latency_ms.clone()))
            .expect("register read_latency_ms");
        registry
            .register(Box::new(backlog_gauge.clone()))
            .expect("register backlog_gauge");
        registry
            .register(Box::new(disk_usage_bytes.clone()))
            .expect("register disk_usage_bytes");
        registry
            .register(Box::new(attestation_status.clone()))
            .expect("register attestation_status");
        attestation_status.set(attestation_configured as i64);

        Self {
            registry,
            appends_total,
            append_errors_total,
            append_latency_ms,
            read_latency_ms,
            backlog_gauge,
            disk_usage_bytes,
            attestation_status,
        }
    }

    fn render(&self) -> anyhow::Result<String> {
        let mut buffer = Vec::new();
        let encoder = prometheus::TextEncoder::new();
        encoder.encode(&self.registry.gather(), &mut buffer)?;
        Ok(String::from_utf8_lossy(&buffer).into_owned())
    }
}

#[derive(Clone)]
struct StatusState {
    metrics: DaemonMetrics,
    log: AppendLog,
    attestation_configured: bool,
}

#[derive(Serialize)]
struct HealthReport {
    status: &'static str,
    backlog: i64,
    log_length: usize,
    disk_usage_bytes: u64,
    attestation_configured: bool,
}

/// Ledgerd command line.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Increase output verbosity.
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Override log level (e.g. info, debug, trace).
    #[arg(long, env = "LEDGER_LOG_LEVEL")]
    log_level: Option<String>,
    /// HTTP bind address for metrics and health endpoints.
    #[arg(
        long,
        env = "LEDGER_STATUS_ADDR",
        default_value = "127.0.0.1:9090",
        help = "Bind address for /metrics, /healthz, and /readyz"
    )]
    status_addr: String,
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
    let level = cli
        .log_level
        .as_deref()
        .map(|lvl| lvl.to_ascii_uppercase())
        .map(|lvl| match lvl.as_str() {
            "TRACE" => Level::TRACE,
            "DEBUG" => Level::DEBUG,
            "INFO" => Level::INFO,
            _ => Level::INFO,
        })
        .unwrap_or_else(|| match cli.verbose {
            0 => Level::INFO,
            1 => Level::DEBUG,
            _ => Level::TRACE,
        });
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.as_str()));
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_env_filter(env_filter)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let registry = load_registry(&cli.registry).await?;
    let transport_config = build_transport_config(&cli.transport)?;
    let transport = bind_transport(registry.clone(), transport_config.clone()).await?;

    match cli.command {
        Commands::Daemon { checkpoint } => {
            daemon(
                checkpoint,
                transport,
                registry.clone(),
                cli.status_addr,
                &transport_config,
            )
            .await?
        }
        Commands::Append { file } => append_from_file(file, transport, &registry).await?,
        Commands::Read { offset, limit } => {
            read_entries(offset, limit, transport, &registry).await?
        }
    }
    Ok(())
}

async fn daemon(
    checkpoint_interval: usize,
    transport: std::sync::Arc<dyn Transport>,
    registry: ChannelRegistry,
    status_addr: String,
    transport_config: &TransportConfig,
) -> anyhow::Result<()> {
    let attestation_configured = transport_config.selected.attestation.is_some();
    let metrics = DaemonMetrics::new(attestation_configured);
    let mut writer = CheckpointWriter::new();
    let mut rx = transport.subscribe().await?;
    let log = AppendLog::new();
    let status_state = std::sync::Arc::new(StatusState {
        metrics: metrics.clone(),
        log: log.clone(),
        attestation_configured,
    });
    let status_listener = tokio::net::TcpListener::bind(&status_addr).await?;
    let status_addr = status_listener.local_addr()?;
    let _status_handle = tokio::spawn(start_status_server(status_listener, status_state));
    info!("ledgerd daemon started");
    info!("status/metrics server listening on {}", status_addr);
    loop {
        let backlog = rx.len() as i64;
        metrics.backlog_gauge.set(backlog);
        let env = rx.recv().await?;
        let span = tracing::info_span!(
            "daemon_append",
            channel = %env.header.channel,
            backlog = backlog,
            offset = tracing::field::Empty
        );
        let _guard = span.enter();
        let start = std::time::Instant::now();
        let append_res = log.append_with_index(env.clone(), &registry);
        let latency = start.elapsed().as_millis() as f64;
        match append_res {
            Ok(idx) => {
                span.record("offset", &(idx as u64));
                metrics
                    .appends_total
                    .with_label_values(&[env.header.channel.as_str()])
                    .inc();
                metrics
                    .append_latency_ms
                    .with_label_values(&[env.header.channel.as_str()])
                    .observe(latency);
            }
            Err(err) => {
                metrics
                    .append_errors_total
                    .with_label_values(&[env.header.channel.as_str()])
                    .inc();
                tracing::error!(error = %err, "append failed");
                continue;
            }
        }
        if let Some(usage) = log.storage_usage_bytes() {
            metrics.disk_usage_bytes.set(usage as i64);
        }
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
    registry: &ChannelRegistry,
) -> anyhow::Result<()> {
    let data = tokio::fs::read(&path).await?;
    let mut env: ledger_spec::Envelope = serde_json::from_slice(&data)?;
    if registry.policy_for(env.header.channel.as_str()).is_none() {
        anyhow::bail!(
            "channel {} not present in registry",
            env.header.channel.as_str()
        );
    }
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
    registry: &ChannelRegistry,
) -> anyhow::Result<()> {
    let span = tracing::info_span!(
        "cli_read",
        offset = offset as u64,
        limit = limit as u64,
        latency_ms = tracing::field::Empty
    );
    let _guard = span.enter();
    let start = std::time::Instant::now();
    let items = transport.read(offset, limit).await?;
    let elapsed = start.elapsed().as_millis() as u64;
    span.record("latency_ms", &elapsed);
    for env in items {
        if registry.policy_for(env.header.channel.as_str()).is_none() {
            anyhow::bail!(
                "channel {} not present in registry",
                env.header.channel.as_str()
            );
        }
        println!(
            "channel={} ts={} payload={}",
            env.header.channel, env.header.timestamp, env.body.payload
        );
    }
    Ok(())
}

async fn start_status_server(
    listener: tokio::net::TcpListener,
    state: std::sync::Arc<StatusState>,
) {
    let app = Router::new()
        .route("/metrics", get(metrics_endpoint))
        .route("/healthz", get(health_endpoint))
        .route("/readyz", get(ready_endpoint))
        .with_state(state);

    if let Err(err) = axum::serve(listener, app.into_make_service()).await {
        tracing::warn!(error = %err, "status server terminated");
    }
}

fn current_health(state: &StatusState, status: &'static str) -> HealthReport {
    HealthReport {
        status,
        backlog: state.metrics.backlog_gauge.get(),
        log_length: state.log.len(),
        disk_usage_bytes: state.metrics.disk_usage_bytes.get() as u64,
        attestation_configured: state.attestation_configured,
    }
}

async fn metrics_endpoint(State(state): State<std::sync::Arc<StatusState>>) -> impl IntoResponse {
    match state.metrics.render() {
        Ok(body) => (StatusCode::OK, body).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to encode metrics: {err}"),
        )
            .into_response(),
    }
}

async fn health_endpoint(State(state): State<std::sync::Arc<StatusState>>) -> impl IntoResponse {
    Json(current_health(&state, "ok"))
}

async fn ready_endpoint(State(state): State<std::sync::Arc<StatusState>>) -> impl IntoResponse {
    Json(current_health(&state, "ready"))
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
