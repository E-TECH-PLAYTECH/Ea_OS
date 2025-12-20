//! Brainstem ledger MVP orchestration: single-writer append, CAS payload store,
//! Merkle receipts, domain indexes, and query surface with proofs.
//!
//! This module intentionally keeps all data in-process without dynamic code
//! loading or shared memory. All mutation is single-writer through `Ledger`.

use std::collections::HashMap;
use std::sync::Arc;

use blake3::Hasher;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::{
    envelope_hash, hash_body, AppendLog, ChannelRegistry, Envelope, MerkleReceipt, ProofNode,
    ProofPosition,
};

/// Content-addressed payload store (blake3 digest).
#[derive(Debug, Default, Clone)]
pub struct ContentStore {
    inner: Arc<RwLock<HashMap<[u8; 32], Vec<u8>>>>,
}

impl ContentStore {
    /// Store bytes and return their digest.
    pub fn put(&self, bytes: Vec<u8>) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(&bytes);
        let digest = *hasher.finalize().as_bytes();
        self.inner.write().insert(digest, bytes);
        digest
    }

    /// Store bytes with a precomputed digest.
    pub fn put_with_digest(&self, digest: [u8; 32], bytes: Vec<u8>) {
        self.inner.write().insert(digest, bytes);
    }

    /// Fetch bytes by digest.
    pub fn get(&self, digest: &[u8; 32]) -> Option<Vec<u8>> {
        self.inner.read().get(digest).cloned()
    }
}

/// Indexes for domain/routing lookups.
#[derive(Debug, Default, Clone)]
pub struct DomainIndex {
    by_channel: Arc<RwLock<HashMap<String, Vec<usize>>>>,
    by_payload_type: Arc<RwLock<HashMap<String, Vec<usize>>>>,
}

impl DomainIndex {
    fn index(&self, env: &Envelope, idx: usize) {
        self.by_channel
            .write()
            .entry(env.header.channel.clone())
            .or_default()
            .push(idx);
        if let Some(pt) = env.body.payload_type.as_ref() {
            self.by_payload_type
                .write()
                .entry(pt.clone())
                .or_default()
                .push(idx);
        }
    }

    /// Fetch offsets for a channel.
    pub fn offsets_for_channel(&self, channel: &str) -> Vec<usize> {
        self.by_channel
            .read()
            .get(channel)
            .cloned()
            .unwrap_or_default()
    }
}

/// Receipt bundle emitted after append.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppendReceipt {
    /// Index of appended envelope.
    pub index: usize,
    /// Inclusion proof.
    pub merkle: MerkleReceipt,
}

/// Alert emitted when validation fails.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Alert {
    /// Validation failure on append.
    ValidationFailed(String),
    /// Query requested nonexistent slice.
    QueryOutOfRange { from: usize, limit: usize },
}

/// Query slice request with proofs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SliceQuery {
    /// Inclusive starting offset.
    pub from: usize,
    /// Maximum number of entries.
    pub limit: usize,
    /// Whether to include payload bytes from the CAS store.
    pub include_payloads: bool,
}

/// Response to a slice query with inclusion proofs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SliceResponse {
    /// Returned envelopes.
    pub envelopes: Vec<Envelope>,
    /// Receipts for each envelope proving log inclusion.
    pub receipts: Vec<MerkleReceipt>,
    /// Optional payload blobs keyed by body hash.
    pub payloads: HashMap<[u8; 32], Vec<u8>>,
}

/// Ledger orchestration façade.
#[derive(Debug, Clone)]
pub struct Ledger {
    registry: ChannelRegistry,
    log: AppendLog,
    store: ContentStore,
    index: DomainIndex,
}

impl Ledger {
    /// Create a new ledger with the given registry.
    pub fn new(registry: ChannelRegistry) -> Self {
        Self {
            registry,
            log: AppendLog::new(),
            store: ContentStore::default(),
            index: DomainIndex::default(),
        }
    }

    /// Append an envelope, enforce invariants, and return a receipt.
    pub fn append(&self, env: Envelope) -> Result<AppendReceipt, Alert> {
        let index = self
            .log
            .append_with_index(env.clone(), &self.registry)
            .map_err(|err| {
                error!("append validation failed: {err:?}");
                Alert::ValidationFailed(err.to_string())
            })?;

        // CAS the payload to allow optional retrieval while keeping log small.
        // Store the canonical body encoding under the deterministic digest.
        let body_bytes = serde_json::to_vec(&env.body).map_err(|err| {
            Alert::ValidationFailed(format!("payload serialization failed: {err}"))
        })?;
        let computed_body_hash = hash_body(&env.body);
        if computed_body_hash != env.header.body_hash {
            return Err(Alert::ValidationFailed("body hash mismatch".into()));
        }
        self.store.put_with_digest(env.header.body_hash, body_bytes);

        self.index.index(&env, index);

        let receipt = self
            .log
            .receipt_for(index)
            .expect("receipt must exist immediately after append");
        info!(
            "append ok channel={} idx={} ts={}",
            env.header.channel, index, env.header.timestamp
        );
        Ok(AppendReceipt {
            index,
            merkle: receipt,
        })
    }

    /// Query a bounded slice with receipts and optional payload blobs.
    pub fn query(&self, req: SliceQuery) -> Result<SliceResponse, Alert> {
        let entries = self.log.read(req.from, req.limit);
        if entries.is_empty() {
            warn!("query out of range from={} limit={}", req.from, req.limit);
            return Err(Alert::QueryOutOfRange {
                from: req.from,
                limit: req.limit,
            });
        }
        let mut receipts = Vec::with_capacity(entries.len());
        let mut payloads = HashMap::new();
        for (i, env) in entries.iter().enumerate() {
            let idx = req.from + i;
            if let Some(receipt) = self.log.receipt_for(idx) {
                receipts.push(receipt);
            }
            if req.include_payloads {
                if let Some(bytes) = self.store.get(&env.header.body_hash) {
                    payloads.insert(env.header.body_hash, bytes);
                }
            }
        }
        Ok(SliceResponse {
            envelopes: entries,
            receipts,
            payloads,
        })
    }

    /// Fetch offsets for a channel (domain index).
    pub fn offsets_for_channel(&self, channel: &str) -> Vec<usize> {
        self.index.offsets_for_channel(channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand_core::OsRng;

    fn registry_with(pk: [u8; 32]) -> ChannelRegistry {
        let mut reg = ChannelRegistry::new();
        reg.upsert(ledger_spec::ChannelSpec {
            name: "test".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![pk],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        reg
    }

    fn make_envelope(sk: &SigningKey, ts: u64, prev: Option<[u8; 32]>) -> (Envelope, [u8; 32]) {
        let body = ledger_spec::EnvelopeBody {
            payload: serde_json::json!({"ts": ts}),
            payload_type: Some("telemetry".into()),
        };
        let body_hash = ledger_spec::hash_body(&body);
        let header = ledger_spec::EnvelopeHeader {
            channel: "test".into(),
            version: 1,
            prev,
            body_hash,
            timestamp: ts,
        };
        let mut env = Envelope {
            header,
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        let sig = sk.sign(&envelope_hash(&env));
        env.signatures.push(ledger_spec::Signature {
            signer: sk.verifying_key().to_bytes(),
            signature: sig.to_bytes(),
        });
        (env, body_hash)
    }

    #[test]
    fn append_and_query_with_receipts() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry_with(sk.verifying_key().to_bytes());
        let ledger = Ledger::new(reg);

        let (env1, _) = make_envelope(&sk, 1, None);
        let (env2, _) = make_envelope(&sk, 2, Some(envelope_hash(&env1)));
        ledger.append(env1).expect("append 1");
        ledger.append(env2).expect("append 2");

        let resp = ledger
            .query(SliceQuery {
                from: 0,
                limit: 10,
                include_payloads: true,
            })
            .expect("query ok");
        assert_eq!(resp.envelopes.len(), 2);
        assert_eq!(resp.receipts.len(), 2);
        assert!(resp.receipts.iter().all(|r| r.verify()));
        assert_eq!(resp.payloads.len(), 2);
    }

    #[test]
    fn alert_on_invalid_append() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry_with(sk.verifying_key().to_bytes());
        let ledger = Ledger::new(reg);
        let (mut env, _) = make_envelope(&sk, 1, None);
        env.signatures.clear();
        let err = ledger.append(env).unwrap_err();
        matches!(err, Alert::ValidationFailed(_));
    }
}
