//! Arda client and orchestrator for ledger-backed command emission, receipts, and replay.
#![deny(missing_docs)]

use std::sync::Arc;

use anyhow::Context;
use ed25519_dalek::{Signer, SigningKey};
use ledger_core::{envelope_hash, signing, AppendLog, MerkleReceipt, ReplayValidator};
use ledger_spec::{
    Envelope, EnvelopeBody, EnvelopeHeader, SchemaVersion, Timestamp, ValidationError,
};
use ledger_transport::Transport;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info};

/// Default schema version for Arda envelopes.
pub const DEFAULT_SCHEMA_VERSION: SchemaVersion = 1;

/// Immutable view of a ledger entry paired with its Merkle receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerViewEntry {
    /// Log index for the envelope.
    pub index: usize,
    /// Envelope content.
    pub envelope: Envelope,
    /// Merkle inclusion receipt.
    pub receipt: MerkleReceipt,
}

/// Orchestrator that wires a transport to a local log, emits commands, and validates replay.
#[derive(Clone)]
pub struct ArdaOrchestrator {
    transport: Arc<dyn Transport>,
    log: AppendLog,
    registry: ledger_spec::ChannelRegistry,
    validator: ReplayValidator,
    signing_key: Arc<SigningKey>,
    schema_version: SchemaVersion,
    channels: Arc<Vec<String>>,
    view_tx: broadcast::Sender<LedgerViewEntry>,
    tail_cache: Arc<RwLock<Option<[u8; 32]>>>,
}

impl ArdaOrchestrator {
    /// Create a new orchestrator bound to the provided transport.
    pub fn new(
        transport: Arc<dyn Transport>,
        registry: ledger_spec::ChannelRegistry,
        signing_key: SigningKey,
        channels: Vec<String>,
        schema_version: SchemaVersion,
    ) -> Self {
        let validator = ReplayValidator::new(registry.clone());
        let (tx, _) = broadcast::channel(1024);
        Self {
            transport,
            log: AppendLog::new(),
            registry,
            validator,
            signing_key: Arc::new(signing_key),
            schema_version,
            channels: Arc::new(channels),
            view_tx: tx,
            tail_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Hydrate the local append log from the transport for deterministic replay.
    pub async fn hydrate(&self, page_size: usize) -> anyhow::Result<Vec<LedgerViewEntry>> {
        let mut offset = 0usize;
        let mut applied = Vec::new();
        loop {
            let batch = self
                .transport
                .read(offset, page_size)
                .await
                .with_context(|| format!("hydrate read offset={offset} limit={page_size}"))?;
            if batch.is_empty() {
                break;
            }
            let batch_len = batch.len();
            for env in batch {
                let entry = self.append_local(env)?;
                applied.push(entry);
            }
            offset += batch_len;
        }
        Ok(applied)
    }

    /// Subscribe to the transport and broadcast verified entries to listeners.
    pub async fn start_subscription(&self) -> anyhow::Result<()> {
        let mut rx = self.transport.subscribe().await?;
        let orchestrator = self.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(env) => {
                        let incoming_hash = envelope_hash(&env);
                        if orchestrator.tail_hash() == Some(incoming_hash) {
                            continue;
                        }
                        match orchestrator.append_local(env) {
                            Ok(entry) => {
                                let _ = orchestrator.view_tx.send(entry);
                            }
                            Err(err) => error!("dropping envelope: {err}"),
                        }
                    }
                    Err(err) => {
                        error!("subscription closed: {err}");
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    /// Returns a receiver for ledger view updates.
    pub fn view_updates(&self) -> broadcast::Receiver<LedgerViewEntry> {
        self.view_tx.subscribe()
    }

    /// Submit a command payload onto a target channel and return its receipt.
    pub async fn submit_command(
        &self,
        channel: &str,
        payload: serde_json::Value,
        payload_type: &str,
        timestamp: Timestamp,
    ) -> anyhow::Result<LedgerViewEntry> {
        if !self.channels.is_empty() && !self.channels.contains(&channel.to_string()) {
            anyhow::bail!("channel {channel} is not whitelisted for this orchestrator");
        }
        let prev = self.tail_hash();
        let body = EnvelopeBody {
            payload,
            payload_type: Some(payload_type.to_string()),
        };
        let body_hash = ledger_spec::hash_body(&body);
        let mut envelope = Envelope {
            header: EnvelopeHeader {
                channel: channel.to_string(),
                version: self.schema_version,
                prev,
                body_hash,
                timestamp,
            },
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        signing::sign_envelope(&mut envelope, &self.signing_key);

        self.transport
            .append(envelope.clone())
            .await
            .context("append via transport failed")?;
        let entry = self.append_local(envelope)?;
        info!("submitted command channel={} idx={}", channel, entry.index);
        Ok(entry)
    }

    /// Verify the local log deterministically.
    pub fn replay(&self) -> Result<(), ValidationError> {
        let all = self.log.read(0, self.log.len());
        self.validator.validate_sequence(&all)
    }

    /// Current length of the local log.
    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    fn append_local(&self, env: Envelope) -> Result<LedgerViewEntry, ValidationError> {
        let index = self.log.append_with_index(env.clone(), &self.registry)?;
        let receipt = self
            .log
            .receipt_for(index)
            .expect("receipt should exist for appended envelope");
        *self.tail_cache.write() = Some(envelope_hash(&env));
        Ok(LedgerViewEntry {
            index,
            envelope: env,
            receipt,
        })
    }

    fn tail_hash(&self) -> Option<[u8; 32]> {
        let cached = *self.tail_cache.read();
        if cached.is_some() {
            return cached;
        }
        let len = self.log.len();
        if len == 0 {
            return None;
        }
        let last = self.log.read(len - 1, 1);
        last.first().map(|env| envelope_hash(env))
    }
}

/// Simple UI event for rendering.
#[derive(Debug, Clone)]
pub enum UiEvent {
    /// New ledger entry with proof.
    Ledger(LedgerViewEntry),
    /// Human-readable status string.
    Status(String),
}

/// Minimal UI shell: streams ledger events and accepts commands.
pub struct ArdaUi {
    orchestrator: ArdaOrchestrator,
    updates: broadcast::Receiver<LedgerViewEntry>,
    status_tx: mpsc::Sender<UiEvent>,
}

impl ArdaUi {
    /// Create a UI shell for the orchestrator.
    pub fn new(orchestrator: ArdaOrchestrator) -> (Self, mpsc::Receiver<UiEvent>) {
        let updates = orchestrator.view_updates();
        let (status_tx, status_rx) = mpsc::channel(32);
        (
            Self {
                orchestrator,
                updates,
                status_tx,
            },
            status_rx,
        )
    }

    /// Start pumping ledger updates into the UI channel.
    pub async fn run(mut self) {
        loop {
            tokio::select! {
                Ok(entry) = self.updates.recv() => {
                    let _ = self.status_tx.send(UiEvent::Ledger(entry)).await;
                },
                else => break,
            }
        }
    }

    /// Render helper for CLI callers.
    pub fn render_entry(entry: &LedgerViewEntry) -> String {
        format!(
            "[#{}] channel={} ts={} merkle_root={:x?} leaf={:x?}",
            entry.index,
            entry.envelope.header.channel,
            entry.envelope.header.timestamp,
            entry.receipt.root,
            entry.receipt.leaf
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use ledger_spec::{ChannelPolicy, ChannelSpec};
    use ledger_transport::InVmQueue;
    use rand_core::OsRng;

    fn registry_for(pk: [u8; 32]) -> ledger_spec::ChannelRegistry {
        let mut reg = ledger_spec::ChannelRegistry::new();
        reg.upsert(ChannelSpec {
            name: "arda.commands".into(),
            policy: ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![pk],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        reg
    }

    #[tokio::test]
    async fn submit_and_replay() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let registry = registry_for(signing_key.verifying_key().to_bytes());
        let transport: Arc<dyn Transport> = Arc::new(InVmQueue::with_registry(registry.clone()));
        let orchestrator = ArdaOrchestrator::new(
            transport,
            registry,
            signing_key,
            vec!["arda.commands".into()],
            DEFAULT_SCHEMA_VERSION,
        );
        orchestrator.hydrate(32).await.unwrap();

        let entry = orchestrator
            .submit_command(
                "arda.commands",
                serde_json::json!({"cmd": "ping"}),
                "ea.event.v1",
                1,
            )
            .await
            .unwrap();
        assert!(entry.receipt.verify());
        orchestrator.replay().unwrap();
    }
}
