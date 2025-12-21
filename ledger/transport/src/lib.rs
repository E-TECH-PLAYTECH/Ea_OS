//! Transport adapters: in-VM queue, Unix socket IPC, QUIC/gRPC split adapters,
//! mailbox bridge for enclaves/accelerators, and loopback for single-VM paths.
#![deny(missing_docs)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;
use tonic::{transport::Server, Request, Response, Status};
use tracing::{info, warn};

use ledger_core::{AppendLogStorage, PersistentAppendLog};
use ledger_spec::{hash_attestation_statement, ChannelRegistry, Envelope};

pub mod proto {
    tonic::include_proto!("ledger.transport");
}

/// Transport error.
pub type TransportResult<T> = Result<T, anyhow::Error>;

/// Transport trait for append/read/subscribe semantics.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Append an envelope to the transport.
    async fn append(&self, env: Envelope) -> TransportResult<()>;
    /// Read envelopes starting at offset with limit.
    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>>;
    /// Subscribe to new envelopes (broadcast).
    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>>;
}

const DEFAULT_QUEUE_DEPTH: usize = 1024;

fn temp_log_dir(label: &str) -> PathBuf {
    let mut path = std::env::temp_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    path.push(format!("ledger-transport-{label}-{nanos}"));
    path
}

fn default_persistent_log(label: &str) -> TransportResult<Arc<dyn AppendLogStorage>> {
    let dir = temp_log_dir(label);
    let log = PersistentAppendLog::open(dir)?;
    Ok(Arc::new(log))
}

fn publish_event(tx: &Sender<Envelope>, queue_depth: usize, env: Envelope) -> TransportResult<()> {
    if tx.len() >= queue_depth {
        anyhow::bail!("backpressure: subscriber queue is full");
    }
    tx.send(env)?;
    Ok(())
}

/// Logical domain that publishes capability advertisements.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportDomain {
    /// Ledgerd or brainstem nodes.
    Ledger,
    /// Arda companion runtimes.
    Arda,
    /// Muscle runtimes (TEE or VM).
    Muscle,
}

/// Adapter kinds supported by the transport layer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data")]
pub enum AdapterKind {
    /// In-process loopback for single-VM deployments.
    Loopback,
    /// QUIC or gRPC split between VM and application tiers.
    QuicGrpc {
        /// Endpoint or authority string.
        endpoint: String,
        /// Optional ALPN for the handshake.
        #[serde(default)]
        alpn: Option<String>,
    },
    /// Mailbox/ring buffer for enclave or chip boundaries.
    Mailbox {
        /// Mailbox identifier (path or device id).
        mailbox: String,
        /// Maximum bytes per slot.
        slot_bytes: usize,
        /// Number of slots in the ring buffer.
        slots: usize,
    },
    /// Unix domain sockets.
    UnixIpc {
        /// Socket path.
        path: String,
    },
    /// Enclave proxy placeholder.
    EnclaveProxy,
}

/// Attestation handshake parameters enforced per adapter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttestationHandshake {
    /// Nonce bound into the attestation evidence.
    pub nonce: String,
    /// Expected runtime identity (e.g., TEE measurement).
    pub expected_runtime_id: Option<String>,
    /// Expected attestation statement hash, if pre-shared.
    pub expected_statement_hash: Option<ledger_spec::Hash>,
    /// Evidence presented by the peer (optional for loopback).
    #[serde(default)]
    pub presented: Option<ledger_spec::Attestation>,
}

impl AttestationHandshake {
    /// Verify that the presented attestation satisfies expectations.
    pub fn verify(&self) -> TransportResult<()> {
        if let Some(att) = &self.presented {
            let computed = hash_attestation_statement(&att.statement);
            if let Some(expected) = &self.expected_statement_hash {
                if expected != &computed {
                    anyhow::bail!("attestation statement hash mismatch");
                }
            }
            if let (
                Some(expected_runtime),
                ledger_spec::AttestationKind::Runtime { runtime_id, .. },
            ) = (&self.expected_runtime_id, &att.statement)
            {
                if runtime_id != expected_runtime {
                    anyhow::bail!("attestation runtime id mismatch");
                }
            }
        }
        Ok(())
    }
}

fn hash_from_vec(bytes: &[u8]) -> TransportResult<ledger_spec::Hash> {
    if bytes.len() != 32 {
        anyhow::bail!("expected 32 byte hash, got {}", bytes.len());
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(bytes);
    Ok(hash)
}

fn signature_from_vec(bytes: &[u8]) -> TransportResult<ledger_spec::SignatureBytes> {
    if bytes.len() != 64 {
        anyhow::bail!("expected 64 byte signature, got {}", bytes.len());
    }
    let mut sig = [0u8; 64];
    sig.copy_from_slice(bytes);
    Ok(sig)
}

fn attestation_from_proto(att: proto::Attestation) -> TransportResult<ledger_spec::Attestation> {
    let statement = match att
        .statement
        .and_then(|s| s.kind)
        .ok_or_else(|| anyhow::anyhow!("attestation statement missing"))?
    {
        proto::attestation_kind::Kind::Build(b) => ledger_spec::AttestationKind::Build {
            artifact_hash: hash_from_vec(&b.artifact_hash)?,
            builder: b.builder,
        },
        proto::attestation_kind::Kind::Runtime(r) => ledger_spec::AttestationKind::Runtime {
            runtime_id: r.runtime_id,
            policy_hash: hash_from_vec(&r.policy_hash)?,
        },
        proto::attestation_kind::Kind::Policy(p) => ledger_spec::AttestationKind::Policy {
            bundle_hash: hash_from_vec(&p.bundle_hash)?,
            expires_at: p.expires_at,
        },
        proto::attestation_kind::Kind::Custom(c) => ledger_spec::AttestationKind::Custom {
            label: c.label,
            payload_hash: hash_from_vec(&c.payload_hash)?,
        },
    };

    Ok(ledger_spec::Attestation {
        issuer: hash_from_vec(&att.issuer)?,
        statement_hash: hash_from_vec(&att.statement_hash)?,
        signature: signature_from_vec(&att.signature)?,
        statement,
    })
}

fn attestation_to_proto(att: &ledger_spec::Attestation) -> proto::Attestation {
    let statement_kind = match &att.statement {
        ledger_spec::AttestationKind::Build {
            artifact_hash,
            builder,
        } => proto::attestation_kind::Kind::Build(proto::AttestationBuild {
            artifact_hash: artifact_hash.to_vec(),
            builder: builder.clone(),
        }),
        ledger_spec::AttestationKind::Runtime {
            runtime_id,
            policy_hash,
        } => proto::attestation_kind::Kind::Runtime(proto::AttestationRuntime {
            runtime_id: runtime_id.clone(),
            policy_hash: policy_hash.to_vec(),
        }),
        ledger_spec::AttestationKind::Policy {
            bundle_hash,
            expires_at,
        } => proto::attestation_kind::Kind::Policy(proto::AttestationPolicy {
            bundle_hash: bundle_hash.to_vec(),
            expires_at: *expires_at,
        }),
        ledger_spec::AttestationKind::Custom {
            label,
            payload_hash,
        } => proto::attestation_kind::Kind::Custom(proto::AttestationCustom {
            label: label.clone(),
            payload_hash: payload_hash.to_vec(),
        }),
    };

    proto::Attestation {
        issuer: att.issuer.to_vec(),
        statement: Some(proto::AttestationKind {
            kind: Some(statement_kind),
        }),
        statement_hash: att.statement_hash.to_vec(),
        signature: att.signature.to_vec(),
    }
}

fn envelope_from_proto(env: proto::Envelope) -> TransportResult<Envelope> {
    let header = env
        .header
        .ok_or_else(|| anyhow::anyhow!("envelope header missing"))?;
    let body = env
        .body
        .ok_or_else(|| anyhow::anyhow!("envelope body missing"))?;
    let payload: serde_json::Value = serde_json::from_str(&body.payload_json)?;
    let prev = if header.prev.is_empty() {
        None
    } else {
        Some(hash_from_vec(&header.prev)?)
    };

    Ok(Envelope {
        header: ledger_spec::EnvelopeHeader {
            channel: header.channel,
            version: header.version as u16,
            prev,
            body_hash: hash_from_vec(&header.body_hash)?,
            timestamp: header.timestamp,
        },
        body: ledger_spec::EnvelopeBody {
            payload,
            payload_type: if body.payload_type.is_empty() {
                None
            } else {
                Some(body.payload_type)
            },
        },
        signatures: env
            .signatures
            .into_iter()
            .map(|s| {
                Ok(ledger_spec::Signature {
                    signer: hash_from_vec(&s.signer)?,
                    signature: signature_from_vec(&s.signature)?,
                })
            })
            .collect::<TransportResult<Vec<_>>>()?,
        attestations: env
            .attestations
            .into_iter()
            .map(attestation_from_proto)
            .collect::<TransportResult<Vec<_>>>()?,
    })
}

fn envelope_to_proto(env: &Envelope) -> TransportResult<proto::Envelope> {
    Ok(proto::Envelope {
        header: Some(proto::EnvelopeHeader {
            channel: env.header.channel.clone(),
            version: env.header.version as u32,
            prev: env.header.prev.map(|h| h.to_vec()).unwrap_or_default(),
            body_hash: env.header.body_hash.to_vec(),
            timestamp: env.header.timestamp,
        }),
        body: Some(proto::EnvelopeBody {
            payload_json: env.body.payload.to_string(),
            payload_type: env.body.payload_type.clone().unwrap_or_default(),
        }),
        signatures: env
            .signatures
            .iter()
            .map(|s| proto::Signature {
                signer: s.signer.to_vec(),
                signature: s.signature.to_vec(),
            })
            .collect(),
        attestations: env.attestations.iter().map(attestation_to_proto).collect(),
    })
}

fn handshake_from_proto(
    handshake: Option<proto::Handshake>,
) -> TransportResult<Option<AttestationHandshake>> {
    match handshake {
        None => Ok(None),
        Some(h) => Ok(Some(AttestationHandshake {
            nonce: h.nonce,
            expected_runtime_id: if h.expected_runtime_id.is_empty() {
                None
            } else {
                Some(h.expected_runtime_id)
            },
            expected_statement_hash: if h.expected_statement_hash.is_empty() {
                None
            } else {
                Some(hash_from_vec(&h.expected_statement_hash)?)
            },
            presented: h.presented.map(attestation_from_proto).transpose()?,
        })),
    }
}

fn handshake_to_proto(handshake: &Option<AttestationHandshake>) -> Option<proto::Handshake> {
    handshake.as_ref().map(|h| proto::Handshake {
        nonce: h.nonce.clone(),
        expected_runtime_id: h.expected_runtime_id.clone().unwrap_or_default(),
        expected_statement_hash: h
            .expected_statement_hash
            .map(|h| h.to_vec())
            .unwrap_or_default(),
        presented: h.presented.as_ref().map(attestation_to_proto),
    })
}

fn verify_with_expected(
    expected: &Option<AttestationHandshake>,
    provided: Option<AttestationHandshake>,
) -> TransportResult<()> {
    let mut handshake = match expected {
        Some(template) => {
            let mut h = template.clone();
            if let Some(provided) = provided {
                h.presented = provided.presented;
            }
            h
        }
        None => provided.unwrap_or(AttestationHandshake {
            nonce: String::new(),
            expected_runtime_id: None,
            expected_statement_hash: None,
            presented: None,
        }),
    };
    if handshake.presented.is_none()
        && (handshake.expected_runtime_id.is_some() || handshake.expected_statement_hash.is_some())
    {
        anyhow::bail!("attestation required but not provided");
    }
    handshake.verify()
}

/// Adapter capability advertised on the ledger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdapterCapability {
    /// Adapter kind and parameters.
    pub adapter: AdapterKind,
    /// Optional features (compression, streaming).
    #[serde(default)]
    pub features: Vec<String>,
    /// Optional attestation handshake requirements.
    #[serde(default)]
    pub attestation: Option<AttestationHandshake>,
}

/// Capability advertisement for a node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityAdvertisement {
    /// Logical domain publishing the capability.
    pub domain: TransportDomain,
    /// Supported protocol versions.
    pub supported_versions: Vec<String>,
    /// Maximum envelope size accepted.
    pub max_message_bytes: usize,
    /// Adapters the node can accept.
    pub adapters: Vec<AdapterCapability>,
}

impl CapabilityAdvertisement {
    /// Build a loopback-only advertisement for convenience.
    pub fn loopback(domain: TransportDomain) -> Self {
        Self {
            domain,
            supported_versions: vec!["1.0.x".into()],
            max_message_bytes: 1_048_576,
            adapters: vec![AdapterCapability {
                adapter: AdapterKind::Loopback,
                features: vec!["inproc".into(), "latency-opt".into()],
                attestation: None,
            }],
        }
    }
}

/// In-VM queue transport using broadcast + local log.
#[derive(Debug, Clone)]
pub struct InVmQueue {
    /// Append-only log.
    pub log: Arc<dyn AppendLogStorage>,
    registry: ChannelRegistry,
    tx: Sender<Envelope>,
    queue_depth: usize,
}

impl InVmQueue {
    /// Create new queue.
    pub fn new() -> TransportResult<Self> {
        Self::with_registry(ChannelRegistry::new())
    }

    /// Create a queue with explicit channel registry (policy enforcement).
    pub fn with_registry(registry: ChannelRegistry) -> TransportResult<Self> {
        let log = default_persistent_log("invm")?;
        Self::with_log(log, registry, DEFAULT_QUEUE_DEPTH)
    }

    /// Create a queue backed by a provided log implementation.
    pub fn with_log(
        log: Arc<dyn AppendLogStorage>,
        registry: ChannelRegistry,
        queue_depth: usize,
    ) -> TransportResult<Self> {
        let depth = queue_depth.max(1);
        let (tx, _) = broadcast::channel(depth);
        Ok(Self {
            log,
            registry,
            tx,
            queue_depth: depth,
        })
    }
}

#[async_trait]
impl Transport for InVmQueue {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.log
            .append(env.clone(), &self.registry)
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        publish_event(&self.tx, self.queue_depth, env)
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        Ok(self.log.read(offset, limit))
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        Ok(self.tx.subscribe())
    }
}

/// Loopback adapter built on the in-VM queue with optional attestation.
#[derive(Debug, Clone)]
pub struct Loopback {
    queue: InVmQueue,
    _attestation: Option<AttestationHandshake>,
}

impl Loopback {
    /// Create a loopback adapter with a registry and optional attestation handshake.
    pub fn new(
        registry: ChannelRegistry,
        attestation: Option<AttestationHandshake>,
    ) -> TransportResult<Self> {
        if let Some(handshake) = &attestation {
            handshake.verify()?;
        }
        Ok(Self {
            queue: InVmQueue::with_registry(registry)?,
            _attestation: attestation,
        })
    }
}

#[async_trait]
impl Transport for Loopback {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.queue.append(env).await
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        self.queue.read(offset, limit).await
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        self.queue.subscribe().await
    }
}

/// Unix IPC request/response frames.
#[derive(Debug, Serialize, Deserialize)]
enum IpcRequest {
    Append(Envelope),
    Read { offset: usize, limit: usize },
    Subscribe,
}

/// Server-originated IPC messages.
#[derive(Debug, Serialize, Deserialize)]
enum IpcResponse {
    AppendOk,
    ReadOk(Vec<Envelope>),
    SubscribeAck,
    Error(String),
}

/// Server-originated events for subscribers.
#[derive(Debug, Serialize, Deserialize)]
enum IpcEvent {
    Envelope(Envelope),
}

fn serialize_frame<T: Serialize>(msg: &T) -> TransportResult<Vec<u8>> {
    let body = bincode::serialize(msg)?;
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

async fn read_frame(stream: &mut UnixStream) -> TransportResult<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok(body)
}

/// Unix socket IPC transport (server-side).
pub struct UnixIpc {
    listener: UnixListener,
    log: Arc<dyn AppendLogStorage>,
    broadcast: Sender<Envelope>,
    registry: ledger_spec::ChannelRegistry,
    queue_depth: usize,
}

impl UnixIpc {
    /// Bind a new Unix socket transport.
    pub async fn bind<P: AsRef<Path>>(
        path: P,
        registry: ledger_spec::ChannelRegistry,
    ) -> TransportResult<Self> {
        Self::bind_with_log(
            path,
            registry,
            default_persistent_log("unix-ipc")?,
            DEFAULT_QUEUE_DEPTH,
        )
    }

    /// Bind a Unix socket transport with a provided log.
    pub async fn bind_with_log<P: AsRef<Path>>(
        path: P,
        registry: ledger_spec::ChannelRegistry,
        log: Arc<dyn AppendLogStorage>,
        queue_depth: usize,
    ) -> TransportResult<Self> {
        if let Some(p) = path.as_ref().to_str() {
            let _ = std::fs::remove_file(p);
        }
        let listener = UnixListener::bind(path)?;
        let depth = queue_depth.max(1);
        let (tx, _) = broadcast::channel(depth);
        Ok(Self {
            listener,
            log,
            broadcast: tx,
            registry,
            queue_depth: depth,
        })
    }

    async fn append_env(&self, env: Envelope) -> TransportResult<()> {
        self.log
            .append(env.clone(), &self.registry)
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        publish_event(&self.broadcast, self.queue_depth, env)
    }

    /// Start accepting connections.
    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                match self.listener.accept().await {
                    Ok((mut stream, _addr)) => {
                        info!("unix ipc: client connected");
                        let this = self.clone();
                        tokio::spawn(async move {
                            let res = this.handle_client(&mut stream).await;
                            if let Err(err) = res {
                                warn!("unix ipc client error: {err:?}");
                            }
                        });
                    }
                    Err(err) => {
                        warn!("unix ipc accept error: {err:?}");
                        break;
                    }
                }
            }
        })
    }

    async fn handle_client(self: Arc<Self>, stream: &mut UnixStream) -> TransportResult<()> {
        loop {
            let frame = match read_frame(stream).await {
                Ok(body) => body,
                Err(err) => {
                    warn!("unix ipc read error: {err:?}");
                    break;
                }
            };
            let req: IpcRequest = bincode::deserialize(&frame)?;
            match req {
                IpcRequest::Append(env) => {
                    let result = self.append_env(env);
                    let resp = match result.await {
                        Ok(_) => IpcResponse::AppendOk,
                        Err(err) => IpcResponse::Error(err.to_string()),
                    };
                    let bytes = serialize_frame(&resp)?;
                    if let Err(err) = stream.write_all(&bytes).await {
                        warn!("unix ipc append response error: {err:?}");
                        break;
                    }
                }
                IpcRequest::Read { offset, limit } => {
                    let resp = match self.read(offset, limit).await {
                        Ok(items) => IpcResponse::ReadOk(items),
                        Err(err) => IpcResponse::Error(err.to_string()),
                    };
                    let bytes = serialize_frame(&resp)?;
                    if let Err(err) = stream.write_all(&bytes).await {
                        warn!("unix ipc read response error: {err:?}");
                        break;
                    }
                }
                IpcRequest::Subscribe => {
                    let resp = serialize_frame(&IpcResponse::SubscribeAck)?;
                    if let Err(err) = stream.write_all(&resp).await {
                        warn!("unix ipc subscribe ack error: {err:?}");
                        break;
                    }
                    let mut rx = self.broadcast.subscribe();
                    let mut stream = stream.try_clone()?;
                    tokio::spawn(async move {
                        loop {
                            match rx.recv().await {
                                Ok(env) => {
                                    let evt = serialize_frame(&IpcEvent::Envelope(env));
                                    match evt {
                                        Ok(bytes) => {
                                            if let Err(err) = stream.write_all(&bytes).await {
                                                warn!("unix ipc event send error: {err:?}");
                                                break;
                                            }
                                        }
                                        Err(err) => {
                                            warn!("unix ipc event serialize error: {err:?}");
                                            break;
                                        }
                                    }
                                }
                                Err(err) => {
                                    warn!("unix ipc subscriber error: {err:?}");
                                    break;
                                }
                            }
                        }
                    });
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Transport for UnixIpc {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.append_env(env).await
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        Ok(self.log.read(offset, limit))
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        Ok(self.broadcast.subscribe())
    }
}

/// Unix IPC client transport that talks to a running daemon.
#[derive(Debug, Clone)]
pub struct UnixIpcClient {
    path: String,
    _registry: ChannelRegistry,
}

impl UnixIpcClient {
    /// Connect to an existing Unix IPC listener.
    pub async fn connect(path: String, registry: ChannelRegistry) -> TransportResult<Self> {
        // Try a simple connection to validate the server is reachable.
        let _ = UnixStream::connect(&path).await?;
        Ok(Self {
            path,
            _registry: registry,
        })
    }

    async fn send_request(&self, req: IpcRequest) -> TransportResult<IpcResponse> {
        let mut stream = UnixStream::connect(&self.path).await?;
        let bytes = serialize_frame(&req)?;
        stream.write_all(&bytes).await?;
        let body = read_frame(&mut stream).await?;
        let resp: IpcResponse = bincode::deserialize(&body)?;
        Ok(resp)
    }
}

#[async_trait]
impl Transport for UnixIpcClient {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        match self.send_request(IpcRequest::Append(env)).await? {
            IpcResponse::AppendOk => Ok(()),
            IpcResponse::Error(e) => Err(anyhow::anyhow!(e)),
            other => Err(anyhow::anyhow!(format!(
                "unexpected response for append: {other:?}"
            ))),
        }
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        match self
            .send_request(IpcRequest::Read { offset, limit })
            .await?
        {
            IpcResponse::ReadOk(items) => Ok(items),
            IpcResponse::Error(e) => Err(anyhow::anyhow!(e)),
            other => Err(anyhow::anyhow!(format!(
                "unexpected response for read: {other:?}"
            ))),
        }
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        let mut stream = UnixStream::connect(&self.path).await?;
        let bytes = serialize_frame(&IpcRequest::Subscribe)?;
        stream.write_all(&bytes).await?;
        // Expect an ack
        let resp_frame = read_frame(&mut stream).await?;
        let resp: IpcResponse = bincode::deserialize(&resp_frame)?;
        if !matches!(resp, IpcResponse::SubscribeAck) {
            anyhow::bail!("unexpected subscribe response: {resp:?}");
        }

        let (tx, rx) = broadcast::channel(1024);
        let mut stream = stream;
        tokio::spawn(async move {
            loop {
                let frame = read_frame(&mut stream).await;
                match frame {
                    Ok(body) => match bincode::deserialize::<IpcEvent>(&body) {
                        Ok(IpcEvent::Envelope(env)) => {
                            let _ = tx.send(env);
                        }
                        Err(err) => {
                            warn!("unix ipc client event decode error: {err:?}");
                            break;
                        }
                    },
                    Err(err) => {
                        warn!("unix ipc client subscribe error: {err:?}");
                        break;
                    }
                }
            }
        });
        Ok(rx)
    }
}

/// Enclave proxy stub interface.
pub struct EnclaveProxyStub;

impl EnclaveProxyStub {
    /// Placeholder for enclave-bound append.
    pub async fn append(&self, _env: Envelope) -> TransportResult<()> {
        Err(anyhow::anyhow!("Enclave proxy not implemented"))
    }
}

/// gRPC transport server implementing append/read/subscribe semantics with attestation enforcement.
struct GrpcTransportService {
    log: Arc<dyn AppendLogStorage>,
    broadcast: Sender<Envelope>,
    registry: ChannelRegistry,
    attestation: Option<AttestationHandshake>,
    queue_depth: usize,
}

impl GrpcTransportService {
    fn new(
        log: Arc<dyn AppendLogStorage>,
        registry: ChannelRegistry,
        attestation: Option<AttestationHandshake>,
        queue_depth: usize,
    ) -> Self {
        let depth = queue_depth.max(1);
        let (tx, _) = broadcast::channel(depth);
        Self {
            log,
            broadcast: tx,
            registry,
            attestation,
            queue_depth: depth,
        }
    }
}

#[tonic::async_trait]
impl proto::transport_server::Transport for GrpcTransportService {
    async fn append(
        &self,
        request: Request<proto::AppendRequest>,
    ) -> Result<Response<proto::AppendResponse>, Status> {
        let req = request.into_inner();
        let handshake = handshake_from_proto(req.handshake)
            .map_err(|e| Status::permission_denied(e.to_string()))?;
        verify_with_expected(&self.attestation, handshake)
            .map_err(|e| Status::permission_denied(e.to_string()))?;

        let env = envelope_from_proto(
            req.envelope
                .ok_or_else(|| Status::invalid_argument("missing envelope"))?,
        )
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

        self.log
            .append(env.clone(), &self.registry)
            .map_err(|err| Status::invalid_argument(err.to_string()))?;
        publish_event(&self.broadcast, self.queue_depth, env)
            .map_err(|err| Status::failed_precondition(err.to_string()))?;
        Ok(Response::new(proto::AppendResponse {}))
    }

    type ReadStream = tokio_stream::wrappers::ReceiverStream<Result<proto::Envelope, Status>>;

    async fn read(
        &self,
        request: Request<proto::ReadRequest>,
    ) -> Result<Response<Self::ReadStream>, Status> {
        let req = request.into_inner();
        let handshake = handshake_from_proto(req.handshake)
            .map_err(|e| Status::permission_denied(e.to_string()))?;
        verify_with_expected(&self.attestation, handshake)
            .map_err(|e| Status::permission_denied(e.to_string()))?;
        let items = self.log.read(req.offset as usize, req.limit as usize);
        let (tx, rx) = tokio::sync::mpsc::channel(items.len().max(1));
        for env in items {
            let proto_env = envelope_to_proto(&env)
                .map_err(|e| Status::internal(format!("encode envelope: {e}")))?;
            if tx.send(Ok(proto_env)).await.is_err() {
                break;
            }
        }
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    type SubscribeStream = tokio_stream::wrappers::ReceiverStream<Result<proto::Envelope, Status>>;

    async fn subscribe(
        &self,
        request: Request<proto::SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let handshake = handshake_from_proto(request.into_inner().handshake)
            .map_err(|e| Status::permission_denied(e.to_string()))?;
        verify_with_expected(&self.attestation, handshake)
            .map_err(|e| Status::permission_denied(e.to_string()))?;

        let rx = self.broadcast.subscribe();
        let stream = BroadcastStream::new(rx).filter_map(|res| match res {
            Ok(env) => match envelope_to_proto(&env) {
                Ok(proto) => Some(Ok(proto)),
                Err(err) => Some(Err(Status::internal(err.to_string()))),
            },
            Err(err) => Some(Err(Status::internal(err.to_string()))),
        });
        let (tx, rx) = tokio::sync::mpsc::channel(self.queue_depth);
        tokio::spawn(async move {
            tokio::pin!(stream);
            while let Some(item) = stream.next().await {
                if tx.send(item).await.is_err() {
                    break;
                }
            }
        });
        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }
}

/// Spawn a gRPC server bound to the provided endpoint (host:port).
pub async fn spawn_quic_grpc_server(
    endpoint: String,
    registry: ChannelRegistry,
    attestation: Option<AttestationHandshake>,
) -> TransportResult<(JoinHandle<()>, std::net::SocketAddr)> {
    let listener = tokio::net::TcpListener::bind(&endpoint).await?;
    let addr = listener.local_addr()?;
    let service = GrpcTransportService::new(
        default_persistent_log("quic-grpc-server")?,
        registry,
        attestation,
        DEFAULT_QUEUE_DEPTH,
    );
    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    let handle = tokio::spawn(async move {
        if let Err(err) = Server::builder()
            .add_service(proto::transport_server::TransportServer::new(service))
            .serve_with_incoming(incoming)
            .await
        {
            warn!("gRPC server error: {err:?}");
        }
    });
    Ok((handle, addr))
}

/// QUIC/gRPC client adapter that mirrors queue semantics while enforcing attestation.
#[derive(Clone)]
pub struct QuicGrpcAdapter {
    client: proto::transport_client::TransportClient<tonic::transport::Channel>,
    attestation: Option<AttestationHandshake>,
    queue_depth: usize,
}

impl std::fmt::Debug for QuicGrpcAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuicGrpcAdapter")
            .field("queue_depth", &self.queue_depth)
            .finish()
    }
}

impl QuicGrpcAdapter {
    /// Establish the adapter after validating attestation.
    pub async fn connect(
        endpoint: String,
        attestation: Option<AttestationHandshake>,
    ) -> TransportResult<Self> {
        if let Some(handshake) = &attestation {
            handshake.verify()?;
        }
        let channel = tonic::transport::Endpoint::from_shared(format!("http://{endpoint}"))?
            .connect()
            .await?;
        Ok(Self {
            client: proto::transport_client::TransportClient::new(channel),
            attestation,
            queue_depth: DEFAULT_QUEUE_DEPTH,
        })
    }

    fn handshake(&self) -> Option<proto::Handshake> {
        handshake_to_proto(&self.attestation)
    }
}

#[async_trait]
impl Transport for QuicGrpcAdapter {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        let req = proto::AppendRequest {
            envelope: Some(envelope_to_proto(&env)?),
            handshake: self.handshake(),
        };
        self.client
            .clone()
            .append(Request::new(req))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        Ok(())
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        let req = proto::ReadRequest {
            offset: offset as u64,
            limit: limit as u64,
            handshake: self.handshake(),
        };
        let mut stream = self
            .client
            .clone()
            .read(Request::new(req))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .into_inner();
        let mut out = Vec::new();
        while let Some(item) = stream.next().await {
            let env = envelope_from_proto(item.map_err(|e| anyhow::anyhow!(e.to_string()))?)?;
            out.push(env);
        }
        Ok(out)
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        let req = proto::SubscribeRequest {
            handshake: self.handshake(),
        };
        let mut stream = self
            .client
            .clone()
            .subscribe(Request::new(req))
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .into_inner();
        let (tx, rx) = broadcast::channel(self.queue_depth);
        let depth = self.queue_depth;
        tokio::spawn(async move {
            while let Some(msg) = stream.next().await {
                match msg {
                    Ok(env) => match envelope_from_proto(env) {
                        Ok(env) => {
                            if tx.len() < depth {
                                let _ = tx.send(env);
                            }
                        }
                        Err(err) => {
                            warn!("gRPC subscribe envelope decode error: {err:?}");
                            break;
                        }
                    },
                    Err(err) => {
                        warn!("gRPC subscribe stream error: {err:?}");
                        break;
                    }
                }
            }
        });
        Ok(rx)
    }
}

/// Mailbox transport for enclave/chip boundaries with bounded slots.
#[derive(Debug, Clone)]
pub struct MailboxTransport {
    _mailbox: String,
    slot_bytes: usize,
    slots: usize,
    log: Arc<dyn AppendLogStorage>,
    broadcast: Sender<Envelope>,
    registry: ChannelRegistry,
    buffer: Arc<Mutex<VecDeque<Envelope>>>,
    _attestation: Option<AttestationHandshake>,
    queue_depth: usize,
}

impl MailboxTransport {
    /// Create a mailbox adapter with attestation enforcement.
    pub fn new(
        mailbox: String,
        slot_bytes: usize,
        slots: usize,
        registry: ChannelRegistry,
        attestation: Option<AttestationHandshake>,
    ) -> TransportResult<Self> {
        let log = default_persistent_log("mailbox")?;
        Self::with_log(
            mailbox,
            slot_bytes,
            slots,
            registry,
            attestation,
            log,
            DEFAULT_QUEUE_DEPTH,
        )
    }

    /// Create a mailbox adapter with an explicit log and queue depth.
    pub fn with_log(
        mailbox: String,
        slot_bytes: usize,
        slots: usize,
        registry: ChannelRegistry,
        attestation: Option<AttestationHandshake>,
        log: Arc<dyn AppendLogStorage>,
        queue_depth: usize,
    ) -> TransportResult<Self> {
        if let Some(handshake) = &attestation {
            handshake.verify()?;
        }
        let depth = queue_depth.max(1);
        let (tx, _) = broadcast::channel(depth);
        Ok(Self {
            _mailbox: mailbox,
            slot_bytes,
            slots,
            log,
            broadcast: tx,
            registry,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(slots))),
            _attestation: attestation,
            queue_depth: depth,
        })
    }

    fn enforce_mailbox_limits(&self, env: &Envelope) -> TransportResult<()> {
        let serialized = bincode::serialize(env)?;
        if serialized.len() > self.slot_bytes {
            anyhow::bail!(
                "envelope exceeds mailbox slot: {} > {} bytes",
                serialized.len(),
                self.slot_bytes
            );
        }
        Ok(())
    }
}

#[async_trait]
impl Transport for MailboxTransport {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.enforce_mailbox_limits(&env)?;
        self.log
            .append(env.clone(), &self.registry)
            .map_err(|err| anyhow::anyhow!(err.to_string()))?;
        {
            let mut buf = self.buffer.lock().await;
            if buf.len() == self.slots {
                anyhow::bail!("mailbox buffer full");
            }
            buf.push_back(env.clone());
        }
        publish_event(&self.broadcast, self.queue_depth, env)
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        Ok(self.log.read(offset, limit))
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        Ok(self.broadcast.subscribe())
    }
}

/// Transport configuration used by orchestrators to bind without workflow changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportConfig {
    /// Capability advertisement to emit or consume.
    pub advertisement: CapabilityAdvertisement,
    /// Adapter selected after negotiation.
    pub selected: AdapterCapability,
}

impl TransportConfig {
    /// Build a loopback configuration with defaults.
    pub fn loopback(domain: TransportDomain) -> Self {
        let advertisement = CapabilityAdvertisement::loopback(domain);
        let selected = advertisement
            .adapters
            .first()
            .expect("loopback adapter should exist")
            .clone();
        Self {
            advertisement,
            selected,
        }
    }
}

impl From<CapabilityAdvertisement> for ledger_spec::events::TransportCapability {
    fn from(value: CapabilityAdvertisement) -> Self {
        let domain = match value.domain {
            TransportDomain::Ledger => ledger_spec::events::CapabilityDomain::Ledger,
            TransportDomain::Arda => ledger_spec::events::CapabilityDomain::Arda,
            TransportDomain::Muscle => ledger_spec::events::CapabilityDomain::Muscle,
        };
        let adapters = value.adapters.into_iter().map(|a| a.into()).collect();
        ledger_spec::events::TransportCapability {
            domain,
            supported_versions: value.supported_versions,
            max_message_bytes: value.max_message_bytes,
            adapters,
        }
    }
}

impl TryFrom<ledger_spec::events::TransportCapability> for CapabilityAdvertisement {
    type Error = anyhow::Error;

    fn try_from(value: ledger_spec::events::TransportCapability) -> Result<Self, Self::Error> {
        let domain = match value.domain {
            ledger_spec::events::CapabilityDomain::Ledger => TransportDomain::Ledger,
            ledger_spec::events::CapabilityDomain::Arda => TransportDomain::Arda,
            ledger_spec::events::CapabilityDomain::Muscle => TransportDomain::Muscle,
        };
        let adapters = value
            .adapters
            .into_iter()
            .map(AdapterCapability::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            domain,
            supported_versions: value.supported_versions,
            max_message_bytes: value.max_message_bytes,
            adapters,
        })
    }
}

impl From<AdapterCapability> for ledger_spec::events::TransportAdapterCapability {
    fn from(value: AdapterCapability) -> Self {
        ledger_spec::events::TransportAdapterCapability {
            adapter: value.adapter.into(),
            features: value.features,
            attestation: value.attestation.map(|a| a.into()),
        }
    }
}

impl TryFrom<ledger_spec::events::TransportAdapterCapability> for AdapterCapability {
    type Error = anyhow::Error;

    fn try_from(
        value: ledger_spec::events::TransportAdapterCapability,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            adapter: AdapterKind::try_from(value.adapter)?,
            features: value.features,
            attestation: value
                .attestation
                .map(AttestationHandshake::try_from)
                .transpose()?,
        })
    }
}

impl From<AdapterKind> for ledger_spec::events::CapabilityAdapterKind {
    fn from(value: AdapterKind) -> Self {
        match value {
            AdapterKind::Loopback => ledger_spec::events::CapabilityAdapterKind::Loopback,
            AdapterKind::QuicGrpc { endpoint, alpn } => {
                ledger_spec::events::CapabilityAdapterKind::QuicGrpc { endpoint, alpn }
            }
            AdapterKind::Mailbox {
                mailbox,
                slot_bytes,
                slots,
            } => ledger_spec::events::CapabilityAdapterKind::Mailbox {
                mailbox,
                slot_bytes,
                slots,
            },
            AdapterKind::UnixIpc { path } => {
                ledger_spec::events::CapabilityAdapterKind::UnixIpc { path }
            }
            AdapterKind::EnclaveProxy => ledger_spec::events::CapabilityAdapterKind::EnclaveProxy,
        }
    }
}

impl TryFrom<ledger_spec::events::CapabilityAdapterKind> for AdapterKind {
    type Error = anyhow::Error;

    fn try_from(value: ledger_spec::events::CapabilityAdapterKind) -> Result<Self, Self::Error> {
        Ok(match value {
            ledger_spec::events::CapabilityAdapterKind::Loopback => AdapterKind::Loopback,
            ledger_spec::events::CapabilityAdapterKind::QuicGrpc { endpoint, alpn } => {
                AdapterKind::QuicGrpc { endpoint, alpn }
            }
            ledger_spec::events::CapabilityAdapterKind::Mailbox {
                mailbox,
                slot_bytes,
                slots,
            } => AdapterKind::Mailbox {
                mailbox,
                slot_bytes,
                slots,
            },
            ledger_spec::events::CapabilityAdapterKind::UnixIpc { path } => {
                AdapterKind::UnixIpc { path }
            }
            ledger_spec::events::CapabilityAdapterKind::EnclaveProxy => AdapterKind::EnclaveProxy,
        })
    }
}

impl From<AttestationHandshake> for ledger_spec::events::CapabilityAttestation {
    fn from(value: AttestationHandshake) -> Self {
        ledger_spec::events::CapabilityAttestation {
            nonce: value.nonce,
            expected_runtime_id: value.expected_runtime_id,
            expected_statement_hash: value.expected_statement_hash,
            presented: value.presented,
        }
    }
}

impl TryFrom<ledger_spec::events::CapabilityAttestation> for AttestationHandshake {
    type Error = anyhow::Error;

    fn try_from(value: ledger_spec::events::CapabilityAttestation) -> Result<Self, Self::Error> {
        Ok(Self {
            nonce: value.nonce,
            expected_runtime_id: value.expected_runtime_id,
            expected_statement_hash: value.expected_statement_hash,
            presented: value.presented,
        })
    }
}

/// Bind a concrete transport implementation from configuration.
pub async fn bind_transport(
    registry: ChannelRegistry,
    cfg: TransportConfig,
) -> TransportResult<Arc<dyn Transport>> {
    match cfg.selected.adapter {
        AdapterKind::Loopback => {
            let att = cfg.selected.attestation;
            let loopback = Loopback::new(registry, att)?;
            Ok(Arc::new(loopback))
        }
        AdapterKind::QuicGrpc { endpoint, .. } => {
            let att = cfg.selected.attestation;
            let adapter = QuicGrpcAdapter::connect(endpoint, att).await?;
            Ok(Arc::new(adapter))
        }
        AdapterKind::Mailbox {
            mailbox,
            slot_bytes,
            slots,
        } => {
            let att = cfg.selected.attestation;
            let adapter = MailboxTransport::new(mailbox, slot_bytes, slots, registry, att)?;
            Ok(Arc::new(adapter))
        }
        AdapterKind::UnixIpc { path } => match UnixStream::connect(&path).await {
            Ok(_) => {
                let client = UnixIpcClient::connect(path, registry).await?;
                Ok(Arc::new(client))
            }
            Err(_) => {
                let ipc = Arc::new(UnixIpc::bind(path, registry).await?);
                let _handle = ipc.clone().start();
                Ok(ipc)
            }
        },
        AdapterKind::EnclaveProxy => {
            Err(anyhow::anyhow!("enclave proxy adapter not yet implemented"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use ledger_core::{signing, AppendLog};
    use ledger_spec::envelope_hash;
    use rand_core::OsRng;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    fn sample_env(sk: &SigningKey, ts: u64, prev: Option<ledger_spec::Hash>) -> Envelope {
        let body = ledger_spec::EnvelopeBody {
            payload: serde_json::json!({"ts": ts}),
            payload_type: Some("test".into()),
        };
        let body_hash = ledger_spec::hash_body(&body);
        let mut env = Envelope {
            header: ledger_spec::EnvelopeHeader {
                channel: "muscle_io".into(),
                version: 1,
                prev,
                body_hash,
                timestamp: ts,
            },
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        signing::sign_envelope(&mut env, sk);
        env
    }

    fn runtime_attestation(runtime_id: &str) -> ledger_spec::Attestation {
        let statement = ledger_spec::AttestationKind::Runtime {
            runtime_id: runtime_id.into(),
            policy_hash: [0xCD; 32],
        };
        let mut att = ledger_spec::Attestation {
            issuer: [0u8; 32],
            statement: statement.clone(),
            statement_hash: hash_attestation_statement(&statement),
            signature: [0u8; 64],
        };
        let sk = SigningKey::generate(&mut OsRng);
        ledger_core::signing::sign_attestation(&mut att, &sk);
        att
    }

    #[tokio::test]
    async fn in_vm_queue_roundtrip() {
        let sk = SigningKey::generate(&mut OsRng);
        let queue = InVmQueue::new().unwrap();
        let env = sample_env(&sk, 1, None);
        let prev_hash = envelope_hash(&env);
        queue.append(env.clone()).await.unwrap();
        let fetched = queue.read(0, 10).await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].header.timestamp, 1);
        let mut rx = queue.subscribe().await.unwrap();
        queue
            .append(sample_env(&sk, 2, Some(prev_hash)))
            .await
            .unwrap();
        let recv = rx.recv().await.unwrap();
        assert_eq!(recv.header.timestamp, 2);
    }

    #[tokio::test]
    async fn attestation_handshake_verifies_runtime() {
        let statement = ledger_spec::AttestationKind::Runtime {
            runtime_id: "enclave-0".into(),
            policy_hash: [0xAB; 32],
        };
        let mut att = ledger_spec::Attestation {
            issuer: [0u8; 32],
            statement: statement.clone(),
            statement_hash: hash_attestation_statement(&statement),
            signature: [0u8; 64],
        };
        let sk = SigningKey::generate(&mut OsRng);
        ledger_core::signing::sign_attestation(&mut att, &sk);

        let handshake = AttestationHandshake {
            nonce: "n-123".into(),
            expected_runtime_id: Some("enclave-0".into()),
            expected_statement_hash: Some(att.statement_hash),
            presented: Some(att.clone()),
        };
        handshake.verify().unwrap();

        let bad_runtime = AttestationHandshake {
            nonce: "n-123".into(),
            expected_runtime_id: Some("enclave-1".into()),
            expected_statement_hash: Some(att.statement_hash),
            presented: Some(att),
        };
        assert!(bad_runtime.verify().is_err());
    }

    #[tokio::test]
    async fn bind_loopback_from_config() {
        let cfg = TransportConfig::loopback(TransportDomain::Ledger);
        let transport = bind_transport(ChannelRegistry::new(), cfg).await.unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_env(&sk, 1, None);
        transport.append(env.clone()).await.unwrap();
        let out = transport.read(0, 1).await.unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].header.timestamp, 1);
    }

    #[test]
    fn advertisement_roundtrip() {
        let cap = CapabilityAdvertisement {
            domain: TransportDomain::Arda,
            supported_versions: vec!["1.0.x".into()],
            max_message_bytes: 1024,
            adapters: vec![AdapterCapability {
                adapter: AdapterKind::Mailbox {
                    mailbox: "/dev/mailbox0".into(),
                    slot_bytes: 2048,
                    slots: 8,
                },
                features: vec!["sealed".into()],
                attestation: None,
            }],
        };
        let spec_cap: ledger_spec::events::TransportCapability = cap.clone().into();
        let roundtrip = CapabilityAdvertisement::try_from(spec_cap).unwrap();
        assert_eq!(roundtrip.domain, cap.domain);
        assert_eq!(roundtrip.adapters.len(), 1);
    }

    #[tokio::test]
    async fn in_vm_queue_backpressure() {
        let sk = SigningKey::generate(&mut OsRng);
        let log = Arc::new(AppendLog::new());
        let queue = InVmQueue::with_log(log, ChannelRegistry::new(), 1).unwrap();
        let first = sample_env(&sk, 1, None);
        queue.append(first.clone()).await.unwrap();
        let err = queue
            .append(sample_env(&sk, 2, Some(envelope_hash(&first))))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("backpressure"));
    }

    #[tokio::test]
    async fn mailbox_overflow_errors() {
        let sk = SigningKey::generate(&mut OsRng);
        let log = Arc::new(AppendLog::new());
        let mailbox =
            MailboxTransport::with_log("mb0".into(), 4096, 1, ChannelRegistry::new(), None, log, 4)
                .unwrap();
        let first = sample_env(&sk, 1, None);
        mailbox.append(first.clone()).await.unwrap();
        let err = mailbox
            .append(sample_env(&sk, 2, Some(envelope_hash(&first))))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("buffer full"));
    }

    #[tokio::test]
    async fn quic_grpc_append_read_roundtrip() {
        let registry = ChannelRegistry::new();
        let att = runtime_attestation("runtime-a");
        let server_handshake = Some(AttestationHandshake {
            nonce: "server-n".into(),
            expected_runtime_id: Some("runtime-a".into()),
            expected_statement_hash: Some(att.statement_hash),
            presented: None,
        });
        let (handle, addr) =
            spawn_quic_grpc_server("127.0.0.1:0".into(), registry.clone(), server_handshake)
                .await
                .unwrap();

        // Give the server a moment to start.
        sleep(Duration::from_millis(50)).await;

        let client_handshake = Some(AttestationHandshake {
            nonce: "client-n".into(),
            expected_runtime_id: Some("runtime-a".into()),
            expected_statement_hash: Some(att.statement_hash),
            presented: Some(att.clone()),
        });

        let adapter = QuicGrpcAdapter::connect(format!("{}", addr), client_handshake)
            .await
            .unwrap();

        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_env(&sk, 10, None);
        adapter.append(env.clone()).await.unwrap();

        let mut rx = adapter.subscribe().await.unwrap();
        let items = adapter.read(0, 10).await.unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].header.timestamp, 10);

        // Ensure subscribe yields the append as well.
        adapter
            .append(sample_env(&sk, 20, Some(envelope_hash(&env))))
            .await
            .unwrap();
        let evt = rx.recv().await.unwrap();
        assert_eq!(evt.header.timestamp, 20);

        handle.abort();
    }

    #[tokio::test]
    async fn quic_grpc_attestation_rejects_mismatch() {
        let registry = ChannelRegistry::new();
        let expected_att = runtime_attestation("runtime-expected");
        let server_handshake = Some(AttestationHandshake {
            nonce: "server-n".into(),
            expected_runtime_id: Some("runtime-expected".into()),
            expected_statement_hash: Some(expected_att.statement_hash),
            presented: None,
        });
        let (handle, addr) =
            spawn_quic_grpc_server("127.0.0.1:0".into(), registry.clone(), server_handshake)
                .await
                .unwrap();
        sleep(Duration::from_millis(50)).await;

        let wrong_att = runtime_attestation("runtime-wrong");
        let client_handshake = Some(AttestationHandshake {
            nonce: "client-n".into(),
            expected_runtime_id: Some("runtime-wrong".into()),
            expected_statement_hash: Some(wrong_att.statement_hash),
            presented: Some(wrong_att),
        });

        let adapter = QuicGrpcAdapter::connect(format!("{}", addr), client_handshake)
            .await
            .unwrap();
        let sk = SigningKey::generate(&mut OsRng);
        let env = sample_env(&sk, 1, None);
        let err = adapter.append(env).await.unwrap_err();
        assert!(
            err.to_string().contains("permission") || err.to_string().contains("denied"),
            "unexpected error: {err}"
        );

        handle.abort();
    }
}
