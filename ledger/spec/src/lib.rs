//! Ledger specification types and validation primitives.
//!
//! This crate defines the core envelope schema, channel registry model,
//! and validation rules (hash chaining, signature sets, and attestations).
#![deny(missing_docs)]

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use std::collections::HashMap;
use thiserror::Error;

/// Event and workflow schema layered on top of envelopes.
pub mod events;
/// Declarative policy model shared across ledger components.
pub mod policy;

/// Blake3 hash output (32 bytes).
pub type Hash = [u8; 32];

/// Ed25519 public key bytes (32 bytes).
pub type PublicKey = [u8; 32];

/// Ed25519 signature bytes (64 bytes).
pub type SignatureBytes = [u8; 64];

/// Channel name type.
pub type Channel = String;

/// Monotonic timestamp (unix epoch millis).
pub type Timestamp = u64;

/// Version marker for envelope schema evolution.
pub type SchemaVersion = u16;

/// Envelope body structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvelopeBody {
    /// Free-form JSON payload.
    pub payload: serde_json::Value,
    /// Optional semantic type tag for routing and policy checks.
    pub payload_type: Option<String>,
}

/// Envelope header.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvelopeHeader {
    /// Channel name.
    pub channel: Channel,
    /// Schema version.
    pub version: SchemaVersion,
    /// Hash of the previous envelope in the channel (for hash chaining).
    pub prev: Option<Hash>,
    /// Hash of the serialized body.
    pub body_hash: Hash,
    /// Wall-clock timestamp (monotonic increase enforced by policy).
    pub timestamp: Timestamp,
}

/// Detached signature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Signature {
    /// Signer public key.
    pub signer: PublicKey,
    /// Signature bytes.
    #[serde(with = "BigArray")]
    pub signature: SignatureBytes,
}

/// Attestation statement kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data")]
pub enum AttestationKind {
    /// Build-time attestation of a muscle blob.
    Build {
        /// Hash of the attested artifact stored in CAS.
        artifact_hash: Hash,
        /// Identity of the builder or pipeline producing the artifact.
        builder: String,
    },
    /// Runtime attestation of execution environment.
    Runtime {
        /// Identifier of the runtime environment (e.g., TEE instance).
        runtime_id: String,
        /// Policy hash enforced by the runtime.
        policy_hash: Hash,
    },
    /// Policy distribution bundle.
    Policy {
        /// Hash of the distributed policy bundle.
        bundle_hash: Hash,
        /// Expiration timestamp for the policy bundle.
        expires_at: Timestamp,
    },
    /// Custom attestation with opaque payload hash.
    Custom {
        /// Domain-separated label for the custom attestation.
        label: String,
        /// Hash of the attested payload.
        payload_hash: Hash,
    },
}

/// Attestation attached to an envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Attestation {
    /// Issuer key.
    pub issuer: PublicKey,
    /// Attestation body.
    pub statement: AttestationKind,
    /// Hash of serialized statement (domain separated).
    pub statement_hash: Hash,
    /// Signature over statement hash.
    #[serde(with = "BigArray")]
    pub signature: SignatureBytes,
}

/// Envelope object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Envelope {
    /// Header.
    pub header: EnvelopeHeader,
    /// Body.
    pub body: EnvelopeBody,
    /// Detached signatures for the envelope header+body hash.
    pub signatures: Vec<Signature>,
    /// Additional attestations bound to the envelope.
    pub attestations: Vec<Attestation>,
}

/// Compute the hash of an envelope body.
pub fn hash_body(body: &EnvelopeBody) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(b"ea-ledger:body");
    let encoded = serde_json::to_vec(body)
        .expect("EnvelopeBody serialization should not fail for trusted input");
    hasher.update(&encoded);
    *hasher.finalize().as_bytes()
}

/// Compute the canonical hash of an envelope header.
pub fn hash_header(header: &EnvelopeHeader) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(b"ea-ledger:header");
    let encoded = serde_json::to_vec(header).expect("EnvelopeHeader serialization should not fail");
    hasher.update(&encoded);
    *hasher.finalize().as_bytes()
}

/// Compute the canonical hash of an envelope (header hash + body hash).
pub fn envelope_hash(env: &Envelope) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(b"ea-ledger:envelope");
    hasher.update(&hash_header(&env.header));
    hasher.update(&env.header.body_hash);
    if let Some(prev) = env.header.prev {
        hasher.update(&prev);
    }
    *hasher.finalize().as_bytes()
}

/// Compute a deterministic hash for an attestation statement.
pub fn hash_attestation_statement(statement: &AttestationKind) -> Hash {
    let mut hasher = Hasher::new();
    hasher.update(b"ea-ledger:attestation");
    let encoded = serde_json::to_vec(statement)
        .expect("AttestationKind serialization should not fail for trusted input");
    hasher.update(&encoded);
    *hasher.finalize().as_bytes()
}

/// Channel policy definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelPolicy {
    /// Minimum required distinct signers.
    pub min_signers: usize,
    /// Allowed signers (whitelist). Empty means any.
    pub allowed_signers: Vec<PublicKey>,
    /// Whether attestations are required.
    pub require_attestations: bool,
    /// Enforce monotonically increasing timestamps.
    pub enforce_timestamp_ordering: bool,
}

impl Default for ChannelPolicy {
    fn default() -> Self {
        Self {
            min_signers: 1,
            allowed_signers: Vec::new(),
            require_attestations: false,
            enforce_timestamp_ordering: true,
        }
    }
}

/// Channel registry entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelSpec {
    /// Channel name.
    pub name: Channel,
    /// Policy rules.
    pub policy: ChannelPolicy,
}

/// Registry of channels.
#[derive(Debug, Default, Clone)]
pub struct ChannelRegistry {
    policies: HashMap<Channel, ChannelPolicy>,
}

impl ChannelRegistry {
    /// Create a new registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register or replace a channel spec.
    pub fn upsert(&mut self, spec: ChannelSpec) {
        self.policies.insert(spec.name.clone(), spec.policy);
    }

    /// Fetch a policy for a channel.
    pub fn policy_for(&self, channel: &str) -> Option<&ChannelPolicy> {
        self.policies.get(channel)
    }
}

/// Validation errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    /// Missing chain link.
    #[error("previous hash mismatch")]
    ChainMismatch,
    /// Not enough signatures.
    #[error("insufficient signatures: {0}")]
    InsufficientSignatures(usize),
    /// Signer not allowed.
    #[error("unauthorized signer")]
    UnauthorizedSigner,
    /// Attestation required but absent.
    #[error("missing required attestations")]
    MissingAttestations,
    /// Timestamp regressed.
    #[error("timestamp regression")]
    TimestampRegression,
    /// Body hash mismatch.
    #[error("body hash mismatch")]
    BodyHashMismatch,
    /// Invalid attestation signature.
    #[error("attestation verification failed")]
    AttestationInvalid,
    /// Envelope signature verification failed.
    #[error("signature verification failed")]
    SignatureInvalid,
}

/// Validation context across a channel (previous hash + timestamp).
#[derive(Debug, Default, Clone)]
pub struct ChannelState {
    /// Last seen hash.
    pub last_hash: Option<Hash>,
    /// Last timestamp.
    pub last_timestamp: Option<Timestamp>,
}

/// Verify an envelope against the registry and previous state.
pub fn validate_envelope(
    env: &Envelope,
    registry: &ChannelRegistry,
    prev_state: &ChannelState,
) -> Result<ChannelState, ValidationError> {
    // Body hash check
    let computed_body = hash_body(&env.body);
    if computed_body != env.header.body_hash {
        return Err(ValidationError::BodyHashMismatch);
    }

    // Chain check
    if env.header.prev != prev_state.last_hash {
        return Err(ValidationError::ChainMismatch);
    }

    // Timestamp ordering
    if let Some(last_ts) = prev_state.last_timestamp {
        if env.header.timestamp < last_ts {
            return Err(ValidationError::TimestampRegression);
        }
    }

    // Policy lookup
    let policy = registry
        .policy_for(&env.header.channel)
        .cloned()
        .unwrap_or_default();

    // Signature check
    if env.signatures.len() < policy.min_signers {
        return Err(ValidationError::InsufficientSignatures(
            env.signatures.len(),
        ));
    }
    let env_hash = envelope_hash(env);
    let mut seen_signers = std::collections::HashSet::new();
    for sig in &env.signatures {
        let pk = ed25519_dalek::VerifyingKey::from_bytes(&sig.signer)
            .map_err(|_| ValidationError::SignatureInvalid)?;
        let signature = ed25519_dalek::Signature::from_bytes(&sig.signature);
        pk.verify_strict(&env_hash, &signature)
            .map_err(|_| ValidationError::SignatureInvalid)?;
        if !policy.allowed_signers.is_empty() && !policy.allowed_signers.contains(&sig.signer) {
            return Err(ValidationError::UnauthorizedSigner);
        }
        seen_signers.insert(sig.signer);
    }
    if seen_signers.len() < policy.min_signers {
        return Err(ValidationError::InsufficientSignatures(
            env.signatures.len(),
        ));
    }

    // Attestations
    if policy.require_attestations && env.attestations.is_empty() {
        return Err(ValidationError::MissingAttestations);
    }
    for att in &env.attestations {
        let computed_statement_hash = hash_attestation_statement(&att.statement);
        if att.statement_hash != computed_statement_hash {
            return Err(ValidationError::AttestationInvalid);
        }
        let pk = ed25519_dalek::VerifyingKey::from_bytes(&att.issuer)
            .map_err(|_| ValidationError::AttestationInvalid)?;
        let signature = ed25519_dalek::Signature::from_bytes(&att.signature);
        pk.verify_strict(&att.statement_hash, &signature)
            .map_err(|_| ValidationError::AttestationInvalid)?;
    }

    Ok(ChannelState {
        last_hash: Some(env_hash),
        last_timestamp: Some(env.header.timestamp),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signing_key() -> SigningKey {
        SigningKey::generate(&mut rand_core::OsRng)
    }

    fn base_envelope() -> (Envelope, SigningKey) {
        let sk = signing_key();
        let body = EnvelopeBody {
            payload: serde_json::json!({"hello": "world"}),
            payload_type: Some("test".into()),
        };
        let body_hash = hash_body(&body);
        let header = EnvelopeHeader {
            channel: "muscle_io".into(),
            version: 1,
            prev: None,
            body_hash,
            timestamp: 1,
        };
        let env = Envelope {
            header,
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        };
        (env, sk)
    }

    #[test]
    fn validates_chain_and_signatures() {
        let (mut env, sk) = base_envelope();
        let env_hash = envelope_hash(&env);
        let sig = sk.sign(&env_hash);
        env.signatures.push(Signature {
            signer: sk.verifying_key().to_bytes(),
            signature: sig.to_bytes(),
        });

        let mut registry = ChannelRegistry::new();
        registry.upsert(ChannelSpec {
            name: "muscle_io".into(),
            policy: ChannelPolicy {
                min_signers: 1,
                allowed_signers: vec![],
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });

        let state = validate_envelope(&env, &registry, &ChannelState::default()).unwrap();
        assert!(state.last_hash.is_some());
    }

    #[test]
    fn rejects_bad_body_hash() {
        let (mut env, sk) = base_envelope();
        env.body.payload = serde_json::json!({"tampered": true});
        let env_hash = envelope_hash(&env);
        let sig = sk.sign(&env_hash);
        env.signatures.push(Signature {
            signer: sk.verifying_key().to_bytes(),
            signature: sig.to_bytes(),
        });
        let registry = ChannelRegistry::new();
        let err = validate_envelope(&env, &registry, &ChannelState::default()).unwrap_err();
        assert_eq!(err, ValidationError::BodyHashMismatch);
    }
}
