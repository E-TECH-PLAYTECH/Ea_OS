//! Brainstem ledger MVP: single-writer appender with validation, receipts,
//! Merkle-indexed query API, and content-addressable payload storage.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use blake3::Hasher;
use ledger_spec::{
    envelope_hash, hash_attestation_statement, hash_body, ChannelRegistry, ChannelState, Envelope,
    ValidationError,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{error, warn};

use crate::{AppendLog, CheckpointWriter};

/// Brainstem ledger error surface.
#[derive(Debug, Error)]
pub enum BrainstemError {
    /// Validation failure.
    #[error("validation failed: {0}")]
    Validation(ValidationError),
    /// Payload rejected due to invariants.
    #[error("invariant violation: {0}")]
    InvariantViolation(String),
    /// Storage error.
    #[error("storage error: {0}")]
    Storage(String),
}

/// Structured alert emitted on validation or invariant failures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Alert {
    /// Human-readable summary.
    pub message: String,
    /// Optional underlying validation error.
    pub error: Option<ValidationError>,
}

/// Receipt proving inclusion of an envelope in the append-only log.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    /// Zero-based index in the log.
    pub index: usize,
    /// Hash of the envelope.
    pub envelope_hash: [u8; 32],
    /// Merkle root at the time of issuance.
    pub merkle_root: [u8; 32],
    /// Sibling path for verification.
    pub merkle_proof: Vec<[u8; 32]>,
}

/// Content-addressable store for envelope payloads.
#[derive(Debug, Clone)]
pub struct ContentAddressableStore {
    root: PathBuf,
    max_bytes: usize,
}

impl ContentAddressableStore {
    /// Create or open a store at the provided root path.
    pub fn new(root: impl AsRef<Path>, max_bytes: usize) -> Result<Self, BrainstemError> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root)
            .map_err(|e| BrainstemError::Storage(format!("create store dir: {e}")))?;
        Ok(Self { root, max_bytes })
    }

    /// Persist a serialized payload and return its hash.
    pub fn put(&self, payload: &serde_json::Value) -> Result<[u8; 32], BrainstemError> {
        let bytes = serde_json::to_vec(payload)
            .map_err(|e| BrainstemError::Storage(format!("serialize payload: {e}")))?;
        if bytes.len() > self.max_bytes {
            return Err(BrainstemError::InvariantViolation(format!(
                "payload exceeds limit: {} > {}",
                bytes.len(),
                self.max_bytes
            )));
        }
        let mut hasher = Hasher::new();
        hasher.update(b"ea-ledger:payload");
        hasher.update(&bytes);
        let hash = *hasher.finalize().as_bytes();
        let path = self.root.join(encode_hex(&hash));
        if !path.exists() {
            fs::write(&path, &bytes)
                .map_err(|e| BrainstemError::Storage(format!("write payload: {e}")))?;
        }
        Ok(hash)
    }

    /// Load a payload by hash if present.
    pub fn get(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, BrainstemError> {
        let path = self.root.join(encode_hex(hash));
        if !path.exists() {
            return Ok(None);
        }
        fs::read(&path)
            .map_err(|e| BrainstemError::Storage(format!("read payload: {e}")))
            .map(Some)
    }
}

/// Domain index mapping identifiers to log offsets.
#[derive(Debug, Default, Clone)]
pub struct DomainIndex {
    by_channel: BTreeMap<String, Vec<usize>>,
    by_domain: BTreeMap<String, Vec<usize>>,
}

impl DomainIndex {
    fn index_envelope(&mut self, idx: usize, env: &Envelope) {
        self.by_channel
            .entry(env.header.channel.clone())
            .or_default()
            .push(idx);
        if let Some(domain) = env
            .body
            .payload
            .get("domain")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
        {
            self.by_domain.entry(domain).or_default().push(idx);
        }
    }

    /// Fetch offsets for a given channel.
    pub fn offsets_for_channel(&self, channel: &str) -> &[usize] {
        self.by_channel
            .get(channel)
            .map(|v| v.as_slice())
            .unwrap_or_default()
    }

    /// Fetch offsets for a domain attribute inside payloads.
    pub fn offsets_for_domain(&self, domain: &str) -> &[usize] {
        self.by_domain
            .get(domain)
            .map(|v| v.as_slice())
            .unwrap_or_default()
    }
}

/// Query result containing envelopes and receipts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuerySlice {
    /// Returned envelopes.
    pub entries: Vec<Envelope>,
    /// Receipts proving inclusion.
    pub receipts: Vec<Receipt>,
    /// Merkle root at query time.
    pub merkle_root: Option<[u8; 32]>,
}

/// Brainstem ledger orchestrator.
#[derive(Debug)]
pub struct BrainstemLedger {
    log: AppendLog,
    registry: ChannelRegistry,
    store: ContentAddressableStore,
    checkpoint_writer: CheckpointWriter,
    alerts: Vec<Alert>,
    index: DomainIndex,
}

impl BrainstemLedger {
    /// Create a new brainstem ledger with the provided storage root and registry.
    pub fn new(
        store_root: impl AsRef<Path>,
        registry: ChannelRegistry,
        max_payload_bytes: usize,
        checkpoint_interval: usize,
    ) -> Result<Self, BrainstemError> {
        Ok(Self {
            log: AppendLog::new(),
            registry,
            store: ContentAddressableStore::new(store_root, max_payload_bytes)?,
            checkpoint_writer: {
                let mut w = CheckpointWriter::new();
                w.maybe_checkpoint(&AppendLog::new(), checkpoint_interval);
                w
            },
            alerts: Vec::new(),
            index: DomainIndex::default(),
        })
    }

    /// Append an envelope after validation, invariant checks, and payload storage.
    pub fn append(&mut self, mut env: Envelope) -> Result<Receipt, BrainstemError> {
        enforce_invariants(&env.body.payload)?;
        // Bind body_hash to payload to defend against replay tampering.
        let computed = hash_body(&env.body);
        if computed != env.header.body_hash {
            return self.validation_error(ValidationError::BodyHashMismatch);
        }
        // Validate attestations include statement hash.
        for att in &env.attestations {
            let computed = hash_attestation_statement(&att.statement);
            if computed != att.statement_hash {
                return self.validation_error(ValidationError::AttestationInvalid);
            }
        }
        let env_hash = envelope_hash(&env);
        let last = if self.log.len() == 0 {
            None
        } else {
            self.log.read(self.log.len() - 1, 1).pop()
        };
        let prev_hash = last.as_ref().map(|e| envelope_hash(e));
        let prev_state = ChannelState {
            last_hash: prev_hash,
            last_timestamp: last.as_ref().map(|e| e.header.timestamp),
        };
        if env.header.prev.is_none() {
            env.header.prev = prev_hash;
        }
        ledger_spec::validate_envelope(&env, &self.registry, &prev_state)
            .map_err(BrainstemError::Validation)?;
        // CAS payload storage (single writer ensures deterministic ordering).
        self.store.put(&env.body.payload)?;
        // Apply append to log with policy enforcement.
        self.log
            .append(env.clone(), &self.registry)
            .map_err(BrainstemError::Validation)?;
        let index = self.log.len() - 1;
        self.index.index_envelope(index, &env);
        let merkle_root = self
            .log
            .merkle_root()
            .expect("merkle root must exist after append");
        let merkle_proof = merkle_proof_for(&self.log, index);
        if let Some(cp) = self.checkpoint_writer.maybe_checkpoint(&self.log, 1) {
            if cp.root != merkle_root {
                warn!("checkpoint root drift detected");
            }
        }
        Ok(Receipt {
            index,
            envelope_hash: env_hash,
            merkle_root,
            merkle_proof,
        })
    }

    /// Query a slice of envelopes with receipts proving inclusion.
    pub fn query_with_proofs(&self, offset: usize, limit: usize) -> QuerySlice {
        let entries = self.log.read(offset, limit);
        let merkle_root = self.log.merkle_root();
        let receipts = entries
            .iter()
            .enumerate()
            .map(|(i, env)| {
                let global_idx = offset + i;
                Receipt {
                    index: global_idx,
                    envelope_hash: envelope_hash(env),
                    merkle_root: merkle_root.unwrap_or([0u8; 32]),
                    merkle_proof: merkle_proof_for(&self.log, global_idx),
                }
            })
            .collect();
        QuerySlice {
            entries,
            receipts,
            merkle_root,
        }
    }

    /// Fetch recorded alerts.
    pub fn alerts(&self) -> &[Alert] {
        &self.alerts
    }

    /// Domain index accessor.
    pub fn domain_index(&self) -> &DomainIndex {
        &self.index
    }

    fn validation_error<T>(&mut self, err: ValidationError) -> Result<T, BrainstemError> {
        let alert = Alert {
            message: format!("validation failure: {err}"),
            error: Some(err.clone()),
        };
        error!("{:?}", alert);
        self.alerts.push(alert);
        Err(BrainstemError::Validation(err))
    }
}

fn enforce_invariants(payload: &serde_json::Value) -> Result<(), BrainstemError> {
    if let Some(obj) = payload.as_object() {
        if obj.contains_key("dynamic_code") {
            return Err(BrainstemError::InvariantViolation(
                "dynamic code payloads are not permitted".into(),
            ));
        }
        if obj.contains_key("shared_memory") {
            return Err(BrainstemError::InvariantViolation(
                "shared memory references are not permitted".into(),
            ));
        }
    }
    Ok(())
}

fn merkle_proof_for(log: &AppendLog, index: usize) -> Vec<[u8; 32]> {
    let entries = log.read(0, log.len());
    let mut leaves: Vec<[u8; 32]> = entries.iter().map(envelope_hash).collect();
    if leaves.is_empty() {
        return Vec::new();
    }
    let mut idx = index;
    let mut proof = Vec::new();
    while leaves.len() > 1 {
        let mut next_level = Vec::with_capacity((leaves.len() + 1) / 2);
        for chunk in leaves.chunks(2) {
            let left = chunk[0];
            let right = if chunk.len() == 2 { chunk[1] } else { chunk[0] };
            let mut hasher = Hasher::new();
            hasher.update(b"ea-ledger:merkle");
            hasher.update(&left);
            hasher.update(&right);
            next_level.push(*hasher.finalize().as_bytes());
        }
        if idx % 2 == 0 {
            if idx + 1 < leaves.len() {
                proof.push(leaves[idx + 1]);
            } else {
                proof.push(leaves[idx]);
            }
        } else {
            proof.push(leaves[idx - 1]);
        }
        idx /= 2;
        leaves = next_level;
    }
    proof
}

fn encode_hex(data: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push(LUT[(byte >> 4) as usize] as char);
        out.push(LUT[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand_core::OsRng;

    fn registry(sk: &SigningKey) -> ChannelRegistry {
        let mut registry = ChannelRegistry::new();
        registry.upsert(ledger_spec::ChannelSpec {
            name: "muscle_io".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![sk.verifying_key().to_bytes()],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        registry
    }

    fn env(sk: &SigningKey, ts: u64, domain: &str) -> Envelope {
        let payload = serde_json::json!({ "n": ts, "domain": domain });
        let body = ledger_spec::EnvelopeBody {
            payload,
            payload_type: Some("test".into()),
        };
        let body_hash = hash_body(&body);
        let header = ledger_spec::EnvelopeHeader {
            channel: "muscle_io".into(),
            version: 1,
            prev: None,
            body_hash,
            timestamp: ts,
        };
        let mut env = Envelope {
            header,
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        let env_hash = envelope_hash(&env);
        let sig = sk.sign(&env_hash);
        env.signatures.push(ledger_spec::Signature {
            signer: sk.verifying_key().to_bytes(),
            signature: sig.to_bytes(),
        });
        env
    }

    #[test]
    fn append_and_query_with_receipts() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut ledger =
            BrainstemLedger::new(std::env::temp_dir(), registry(&sk), 8 * 1024 * 1024, 10).unwrap();
        let r1 = ledger.append(env(&sk, 1, "alpha")).unwrap();
        assert_eq!(r1.index, 0);
        let r2 = ledger.append(env(&sk, 2, "beta")).unwrap();
        assert_eq!(r2.index, 1);
        assert_eq!(ledger.domain_index().offsets_for_domain("alpha"), &[0]);
        let slice = ledger.query_with_proofs(0, 2);
        assert_eq!(slice.entries.len(), 2);
        assert_eq!(slice.receipts.len(), 2);
        assert!(slice.merkle_root.is_some());
        assert!(!slice.receipts[0].merkle_proof.is_empty());
        assert!(!slice.receipts[1].merkle_proof.is_empty());
    }

    #[test]
    fn invariant_blocks_dynamic_code() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut ledger =
            BrainstemLedger::new(std::env::temp_dir(), registry(&sk), 8 * 1024 * 1024, 10).unwrap();
        let mut env = env(&sk, 1, "alpha");
        env.body
            .payload
            .as_object_mut()
            .unwrap()
            .insert("dynamic_code".into(), serde_json::json!(true));
        let err = ledger.append(env).unwrap_err();
        matches!(err, BrainstemError::InvariantViolation(_));
    }

    #[test]
    fn emits_alert_on_validation_failure() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut ledger =
            BrainstemLedger::new(std::env::temp_dir(), registry(&sk), 8 * 1024 * 1024, 10).unwrap();
        let mut env = env(&sk, 2, "alpha");
        env.header.prev = Some([0u8; 32]); // break chain
        let _ = ledger.append(env);
        assert!(!ledger.alerts().is_empty());
    }
}
