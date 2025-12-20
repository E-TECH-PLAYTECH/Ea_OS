//! Transport adapters: in-VM queue, Unix socket IPC, QUIC/gRPC split adapters,
//! mailbox bridge for enclaves/accelerators, and loopback for single-VM paths.
#![deny(missing_docs)]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use ledger_core::AppendLog;
use ledger_spec::{hash_attestation_statement, ChannelRegistry, Envelope};

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
    pub log: AppendLog,
    registry: ChannelRegistry,
    tx: Sender<Envelope>,
}

impl InVmQueue {
    /// Create new queue.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            log: AppendLog::new(),
            registry: ChannelRegistry::new(),
            tx,
        }
    }

    /// Create a queue with explicit channel registry (policy enforcement).
    pub fn with_registry(registry: ChannelRegistry) -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            log: AppendLog::new(),
            registry,
            tx,
        }
    }
}

#[async_trait]
impl Transport for InVmQueue {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.log.append(env.clone(), &self.registry)?;
        let _ = self.tx.send(env);
        Ok(())
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
            queue: InVmQueue::with_registry(registry),
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

/// Unix socket IPC transport.
pub struct UnixIpc {
    listener: UnixListener,
    clients: Arc<Mutex<Vec<UnixStream>>>,
    log: AppendLog,
    broadcast: Sender<Envelope>,
    registry: ledger_spec::ChannelRegistry,
}

impl UnixIpc {
    /// Bind a new Unix socket transport.
    pub async fn bind<P: AsRef<Path>>(
        path: P,
        registry: ledger_spec::ChannelRegistry,
    ) -> TransportResult<Self> {
        if let Some(p) = path.as_ref().to_str() {
            let _ = std::fs::remove_file(p);
        }
        let listener = UnixListener::bind(path)?;
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            listener,
            clients: Arc::new(Mutex::new(Vec::new())),
            log: AppendLog::new(),
            broadcast: tx,
            registry,
        })
    }

    /// Start accepting connections.
    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                match self.listener.accept().await {
                    Ok((stream, _addr)) => {
                        info!("unix ipc: client connected");
                        self.clients.lock().await.push(stream);
                    }
                    Err(err) => {
                        warn!("unix ipc accept error: {err:?}");
                        break;
                    }
                }
            }
        })
    }
}

#[async_trait]
impl Transport for UnixIpc {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.log.append(env.clone(), &self.registry)?;
        let bytes = bincode::serialize(&env)?;
        let mut clients = self.clients.lock().await;
        clients.retain_mut(|c| {
            let res = futures::executor::block_on(async {
                c.write_all(&(bytes.len() as u32).to_be_bytes()).await?;
                c.write_all(&bytes).await
            });
            res.is_ok()
        });
        let _ = self.broadcast.send(env);
        Ok(())
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        Ok(self.log.read(offset, limit))
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        Ok(self.broadcast.subscribe())
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

/// QUIC/gRPC adapter that mirrors queue semantics while enforcing attestation.
#[derive(Debug, Clone)]
pub struct QuicGrpcAdapter {
    log: AppendLog,
    broadcast: Sender<Envelope>,
    registry: ChannelRegistry,
    endpoint: String,
    _attestation: Option<AttestationHandshake>,
}

impl QuicGrpcAdapter {
    /// Establish the adapter after validating attestation.
    pub fn connect(
        endpoint: String,
        registry: ChannelRegistry,
        attestation: Option<AttestationHandshake>,
    ) -> TransportResult<Self> {
        if let Some(handshake) = &attestation {
            handshake.verify()?;
        }
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            log: AppendLog::new(),
            broadcast: tx,
            registry,
            endpoint,
            _attestation: attestation,
        })
    }

    /// Endpoint accessor for observability hooks.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

#[async_trait]
impl Transport for QuicGrpcAdapter {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.log.append(env.clone(), &self.registry)?;
        let _ = self.broadcast.send(env);
        Ok(())
    }

    async fn read(&self, offset: usize, limit: usize) -> TransportResult<Vec<Envelope>> {
        Ok(self.log.read(offset, limit))
    }

    async fn subscribe(&self) -> TransportResult<Receiver<Envelope>> {
        Ok(self.broadcast.subscribe())
    }
}

/// Mailbox transport for enclave/chip boundaries with bounded slots.
#[derive(Debug, Clone)]
pub struct MailboxTransport {
    _mailbox: String,
    slot_bytes: usize,
    slots: usize,
    log: AppendLog,
    broadcast: Sender<Envelope>,
    registry: ChannelRegistry,
    buffer: Arc<Mutex<VecDeque<Envelope>>>,
    _attestation: Option<AttestationHandshake>,
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
        if let Some(handshake) = &attestation {
            handshake.verify()?;
        }
        let (tx, _) = broadcast::channel(1024);
        Ok(Self {
            _mailbox: mailbox,
            slot_bytes,
            slots,
            log: AppendLog::new(),
            broadcast: tx,
            registry,
            buffer: Arc::new(Mutex::new(VecDeque::with_capacity(slots))),
            _attestation: attestation,
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
        self.log.append(env.clone(), &self.registry)?;
        {
            let mut buf = self.buffer.lock().await;
            if buf.len() == self.slots {
                buf.pop_front();
            }
            buf.push_back(env.clone());
        }
        let _ = self.broadcast.send(env);
        Ok(())
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
            let adapter = QuicGrpcAdapter::connect(endpoint, registry, att)?;
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
        AdapterKind::UnixIpc { path } => {
            let ipc = Arc::new(UnixIpc::bind(path, registry).await?);
            let _handle = ipc.clone().start();
            Ok(ipc)
        }
        AdapterKind::EnclaveProxy => {
            Err(anyhow::anyhow!("enclave proxy adapter not yet implemented"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use ledger_core::signing;
    use ledger_spec::envelope_hash;
    use rand_core::OsRng;

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

    #[tokio::test]
    async fn in_vm_queue_roundtrip() {
        let sk = SigningKey::generate(&mut OsRng);
        let queue = InVmQueue::new();
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
}
