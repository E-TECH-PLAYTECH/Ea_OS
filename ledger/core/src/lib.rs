//! Ledger core library: envelope signing/verification, append-only log,
//! Merkle segmenter, checkpoint writer, and replay validator.
#![deny(missing_docs)]

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use blake3::Hasher;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use ledger_spec::{
    envelope_hash, hash_body, Attestation, ChannelRegistry, ChannelState, Envelope, EnvelopeBody,
    EnvelopeHeader, Signature, ValidationError,
};

/// Append-only log identifier.
pub type LogId = String;

/// In-memory append-only log with hash chaining and Merkle checkpoints.
#[derive(Debug, Default, Clone)]
pub struct AppendLog {
    entries: Arc<RwLock<Vec<Envelope>>>,
}

impl AppendLog {
    /// Create a new empty log.
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Append an envelope after validation.
    pub fn append(
        &self,
        mut env: Envelope,
        registry: &ChannelRegistry,
    ) -> Result<(), ValidationError> {
        let mut entries = self.entries.write();
        let prev_hash = entries.last().map(envelope_hash);
        if env.header.prev.is_none() {
            env.header.prev = prev_hash;
        }
        let prev_state = ChannelState {
            last_hash: prev_hash,
            last_timestamp: entries.last().map(|e| e.header.timestamp),
        };
        let _ = ledger_spec::validate_envelope(&env, registry, &prev_state)?;
        entries.push(env);
        Ok(())
    }

    /// Read a slice of envelopes.
    pub fn read(&self, offset: usize, limit: usize) -> Vec<Envelope> {
        let entries = self.entries.read();
        entries.iter().skip(offset).take(limit).cloned().collect()
    }

    /// Return the length.
    pub fn len(&self) -> usize {
        self.entries.read().len()
    }

    /// Compute a Merkle root over current entries.
    pub fn merkle_root(&self) -> Option<[u8; 32]> {
        let entries = self.entries.read();
        if entries.is_empty() {
            return None;
        }
        let mut leaves: Vec<[u8; 32]> = entries.iter().map(envelope_hash).collect();
        while leaves.len() > 1 {
            leaves = leaves
                .chunks(2)
                .map(|chunk| {
                    let mut hasher = Hasher::new();
                    hasher.update(b"ea-ledger:merkle");
                    hasher.update(&chunk[0]);
                    if chunk.len() == 2 {
                        hasher.update(&chunk[1]);
                    } else {
                        hasher.update(&chunk[0]);
                    }
                    *hasher.finalize().as_bytes()
                })
                .collect();
        }
        leaves.into_iter().next()
    }
}

/// Checkpoint record capturing merkle root and length.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkpoint {
    /// Log length at checkpoint.
    pub length: usize,
    /// Merkle root.
    pub root: [u8; 32],
}

/// Checkpoint writer produces periodic checkpoints.
#[derive(Debug, Default)]
pub struct CheckpointWriter {
    last_len: usize,
}

impl CheckpointWriter {
    /// Create new writer.
    pub fn new() -> Self {
        Self { last_len: 0 }
    }

    /// Emit a checkpoint if log advanced by at least `interval`.
    pub fn maybe_checkpoint(&mut self, log: &AppendLog, interval: usize) -> Option<Checkpoint> {
        let len = log.len();
        if len >= self.last_len + interval {
            let root = log.merkle_root()?;
            self.last_len = len;
            return Some(Checkpoint { length: len, root });
        }
        None
    }
}

/// Replay validator detects tampering or reordering.
pub struct ReplayValidator {
    registry: ChannelRegistry,
}

impl ReplayValidator {
    /// Create new validator.
    pub fn new(registry: ChannelRegistry) -> Self {
        Self { registry }
    }

    /// Validate a sequence of envelopes starting from empty state.
    pub fn validate_sequence(&self, seq: &[Envelope]) -> Result<(), ValidationError> {
        let mut state = ChannelState::default();
        for env in seq {
            state = ledger_spec::validate_envelope(env, &self.registry, &state)?;
        }
        Ok(())
    }
}

/// Envelope signer and verifier helpers.
pub mod signing {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// Sign an envelope (header/body) with the provided key.
    pub fn sign_envelope(env: &mut Envelope, signer: &SigningKey) {
        let env_hash = envelope_hash(env);
        let sig = signer.sign(&env_hash);
        env.signatures.push(Signature {
            signer: signer.verifying_key().to_bytes(),
            signature: sig.to_bytes(),
        });
    }

    /// Attach an attestation signature over its statement hash.
    pub fn sign_attestation(att: &mut Attestation, signer: &SigningKey) {
        let sig = signer.sign(&att.statement_hash);
        att.signature = sig.to_bytes();
        att.issuer = signer.verifying_key().to_bytes();
    }
}

/// Append-only log segmenter that emits Merkle checkpoints.
#[derive(Debug)]
pub struct MerkleSegmenter {
    window: usize,
    queue: VecDeque<[u8; 32]>,
}

impl MerkleSegmenter {
    /// Create a new segmenter with a fixed window size.
    pub fn new(window: usize) -> Self {
        Self {
            window,
            queue: VecDeque::new(),
        }
    }

    /// Push a new envelope hash and emit a segment root if window filled.
    pub fn push(&mut self, env_hash: [u8; 32]) -> Option<[u8; 32]> {
        self.queue.push_back(env_hash);
        if self.queue.len() == self.window {
            let root = compute_merkle(&self.queue.make_contiguous());
            self.queue.clear();
            Some(root)
        } else {
            None
        }
    }
}

fn compute_merkle(items: &[[u8; 32]]) -> [u8; 32] {
    let mut leaves = items.to_vec();
    if leaves.is_empty() {
        return [0u8; 32];
    }
    while leaves.len() > 1 {
        leaves = leaves
            .chunks(2)
            .map(|chunk| {
                let mut hasher = Hasher::new();
                hasher.update(b"ea-ledger:merkle");
                hasher.update(&chunk[0]);
                if chunk.len() == 2 {
                    hasher.update(&chunk[1]);
                } else {
                    hasher.update(&chunk[0]);
                }
                *hasher.finalize().as_bytes()
            })
            .collect();
    }
    leaves[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand_core::OsRng;

    fn sample_env(prev: Option<[u8; 32]>, ts: u64, sk: &SigningKey) -> Envelope {
        let body = EnvelopeBody {
            payload: serde_json::json!({"n": ts}),
            payload_type: Some("test".into()),
        };
        let body_hash = hash_body(&body);
        let header = EnvelopeHeader {
            channel: "muscle_io".into(),
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
        signing::sign_envelope(&mut env, sk);
        env
    }

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

    #[test]
    fn append_and_checkpoint() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let log = AppendLog::new();
        let mut prev = None;
        for ts in 1..=3 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            log.append(env, &reg).unwrap();
        }
        assert_eq!(log.len(), 3);
        let mut writer = CheckpointWriter::new();
        let cp = writer.maybe_checkpoint(&log, 2).unwrap();
        assert_eq!(cp.length, 3);
        assert!(cp.root.iter().any(|b| *b != 0));
    }

    #[test]
    fn merkle_segmenter_emits_root() {
        let sk = SigningKey::generate(&mut OsRng);
        let mut segmenter = MerkleSegmenter::new(2);
        let mut prev = None;
        for ts in 1..=2 {
            let env = sample_env(prev, ts, &sk);
            prev = Some(envelope_hash(&env));
            let root = segmenter.push(envelope_hash(&env));
            if ts == 2 {
                assert!(root.is_some());
            }
        }
    }

    #[test]
    fn replay_validator_detects_tamper() {
        let sk = SigningKey::generate(&mut OsRng);
        let reg = registry(&sk);
        let validator = ReplayValidator::new(reg);
        let env1 = sample_env(None, 1, &sk);
        let mut env2 = sample_env(Some(envelope_hash(&env1)), 2, &sk);
        // Tamper body without updating hash
        env2.body.payload = serde_json::json!({"n": 99});
        let seq = vec![env1, env2];
        let err = validator.validate_sequence(&seq).unwrap_err();
        assert_eq!(err, ValidationError::BodyHashMismatch);
    }
}
