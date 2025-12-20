//! Transport adapters: in-VM queue, Unix socket IPC, and enclave proxy stub.
#![deny(missing_docs)]

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::broadcast;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use ledger_core::{signing, AppendLog};
use ledger_spec::Envelope;

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

/// In-VM queue transport using broadcast + local log.
#[derive(Debug, Clone)]
pub struct InVmQueue {
    /// Append-only log.
    pub log: AppendLog,
    tx: Sender<Envelope>,
}

impl InVmQueue {
    /// Create new queue.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            log: AppendLog::new(),
            tx,
        }
    }
}

#[async_trait]
impl Transport for InVmQueue {
    async fn append(&self, env: Envelope) -> TransportResult<()> {
        self.log
            .append(env.clone(), &ledger_spec::ChannelRegistry::new())?;
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
impl Transport for Arc<UnixIpc> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand_core::OsRng;

    fn sample_env(sk: &SigningKey, ts: u64) -> Envelope {
        let body = ledger_spec::EnvelopeBody {
            payload: serde_json::json!({"ts": ts}),
            payload_type: Some("test".into()),
        };
        let body_hash = ledger_spec::hash_body(&body);
        let mut env = Envelope {
            header: ledger_spec::EnvelopeHeader {
                channel: "muscle_io".into(),
                version: 1,
                prev: None,
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
        let env = sample_env(&sk, 1);
        queue.append(env.clone()).await.unwrap();
        let fetched = queue.read(0, 10).await.unwrap();
        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].header.timestamp, 1);
        let mut rx = queue.subscribe().await.unwrap();
        queue.append(sample_env(&sk, 2)).await.unwrap();
        let recv = rx.recv().await.unwrap();
        assert_eq!(recv.header.timestamp, 2);
    }
}
