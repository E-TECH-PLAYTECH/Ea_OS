//! Muscle lifecycle manager that enforces register→seal→activate→invoke discipline.
//!
//! The manager consumes ledger events, validates measurements and attestations,
//! updates an in-memory registry, hydrates the CAS with sealed blobs, and emits
//! lifecycle updates or enforcement errors back onto the ledger event stream.

use std::collections::{HashMap, HashSet};

use blake3::Hasher;
use ed25519_dalek::Verifier;
use ledger_spec::events::{
    Audience, ContentRef, DataSensitivity, EventKind, LedgerEvent, LifecycleCommand,
    LifecycleError, LifecycleStage, LifecycleUpdate, MuscleEvent, MuscleRef,
};
use ledger_spec::{
    hash_attestation_statement, Attestation, AttestationKind, Hash, PublicKey, SchemaVersion,
};

use crate::brainstem::ContentStore;

type MuscleKey = (Hash, u64);

/// Registry entry for a muscle instance across its lifecycle.
#[derive(Debug, Clone)]
pub struct MuscleRecord {
    /// Reference to the muscle/version tuple.
    pub reference: MuscleRef,
    /// Expected measurement hash.
    pub measurement: Hash,
    /// Policy tags that must be honored on activation and invocation.
    pub policy_tags: Vec<String>,
    /// Stored sealed blob reference once accepted.
    pub sealed_blob: Option<ContentRef>,
    /// Statement hash of the validated attestation.
    pub attestation: Option<Hash>,
    /// Policy bundle reference applied at activation.
    pub policy: Option<ContentRef>,
    /// Current lifecycle stage.
    pub stage: LifecycleStage,
    /// Last recorded error for observability.
    pub last_error: Option<String>,
}

impl MuscleRecord {
    fn new(reference: MuscleRef, measurement: Hash, policy_tags: Vec<String>) -> Self {
        Self {
            reference,
            measurement,
            policy_tags,
            sealed_blob: None,
            attestation: None,
            policy: None,
            stage: LifecycleStage::Registered,
            last_error: None,
        }
    }
}

/// Lifecycle manager that drives state transitions and enforces attested policy.
#[derive(Debug, Clone)]
pub struct MuscleLifecycleManager {
    registry: HashMap<MuscleKey, MuscleRecord>,
    store: ContentStore,
    issuer: PublicKey,
    lifecycle_channel: String,
    schema_version: SchemaVersion,
}

impl MuscleLifecycleManager {
    /// Create a new lifecycle manager.
    pub fn new(
        store: ContentStore,
        issuer: PublicKey,
        lifecycle_channel: impl Into<String>,
        schema_version: SchemaVersion,
    ) -> Self {
        Self {
            registry: HashMap::new(),
            store,
            issuer,
            lifecycle_channel: lifecycle_channel.into(),
            schema_version,
        }
    }

    /// Put bytes into the CAS and return a content reference.
    pub fn ingest_blob(
        &self,
        bytes: Vec<u8>,
        locator_hint: Option<String>,
        media_type: Option<String>,
    ) -> ContentRef {
        let digest = self.store.put(bytes.clone());
        let locator = locator_hint.unwrap_or_else(|| {
            let hex = blake3::Hash::from_bytes(digest).to_hex();
            format!("cas:{hex}")
        });
        ContentRef {
            locator,
            hash: digest,
            media_type,
            bytes: Some(bytes.len() as u64),
        }
    }

    /// Handle a ledger event and return any lifecycle events that should be appended.
    pub fn handle_event(&mut self, event: &LedgerEvent) -> Vec<LedgerEvent> {
        match &event.kind {
            EventKind::Muscle(MuscleEvent::LifecycleCommand(cmd)) => {
                self.handle_command(cmd, event)
            }
            EventKind::Muscle(MuscleEvent::InvocationRequest {
                muscle,
                policy,
                policy_tags,
                ..
            }) => self.validate_invocation(muscle, policy.as_ref(), policy_tags, event),
            _ => Vec::new(),
        }
    }

    /// Expose a read-only view of the registry for observability or tests.
    pub fn record(&self, muscle: &MuscleRef) -> Option<&MuscleRecord> {
        self.registry.get(&Self::key(muscle))
    }

    fn handle_command(&mut self, cmd: &LifecycleCommand, source: &LedgerEvent) -> Vec<LedgerEvent> {
        match cmd {
            LifecycleCommand::Register {
                muscle,
                measurement,
                manifest,
                policy_tags,
            } => self.handle_register(muscle, *measurement, manifest.clone(), policy_tags, source),
            LifecycleCommand::Seal {
                muscle,
                sealed_blob,
                measurement,
                inline_blob,
            } => self.handle_seal(muscle, sealed_blob, *measurement, inline_blob, source),
            LifecycleCommand::Activate {
                muscle,
                policy,
                policy_tags,
            } => self.handle_activate(muscle, policy.clone(), policy_tags, source),
            LifecycleCommand::Retire { muscle, reason } => {
                self.handle_retire(muscle, reason, source)
            }
        }
    }

    fn handle_register(
        &mut self,
        muscle: &MuscleRef,
        measurement: Hash,
        manifest: Option<ContentRef>,
        policy_tags: &[String],
        source: &LedgerEvent,
    ) -> Vec<LedgerEvent> {
        let key = Self::key(muscle);
        let record = self.registry.entry(key).or_insert_with(|| {
            MuscleRecord::new(muscle.clone(), measurement, policy_tags.to_vec())
        });

        if record.measurement != measurement {
            let reason = format!(
                "measurement mismatch: existing {} new {}",
                fmt_hash(&record.measurement),
                fmt_hash(&measurement)
            );
            return vec![self.error_event(muscle, LifecycleStage::Registered, reason, source)];
        }

        record.stage = LifecycleStage::Registered;
        record.policy_tags = policy_tags.to_vec();
        record.last_error = None;

        let mut attachments = Vec::new();
        if let Some(manifest_ref) = manifest {
            attachments.push(manifest_ref.clone());
        }

        vec![self.lifecycle_event(
            MuscleEvent::LifecycleUpdate(LifecycleUpdate::Registered {
                muscle: muscle.clone(),
                measurement,
                policy_tags: policy_tags.to_vec(),
            }),
            attachments,
            source,
            DataSensitivity::Internal,
        )]
    }

    fn handle_seal(
        &mut self,
        muscle: &MuscleRef,
        sealed_blob: &ContentRef,
        measurement: Hash,
        inline_blob: &Option<Vec<u8>>,
        source: &LedgerEvent,
    ) -> Vec<LedgerEvent> {
        let Some(record) = self.registry.get_mut(&Self::key(muscle)) else {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Registered,
                "muscle not registered".into(),
                source,
            )];
        };

        if record.stage == LifecycleStage::Retired {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Retired,
                "retired muscle cannot be sealed".into(),
                source,
            )];
        }

        if record.measurement != measurement {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Registered,
                "measurement mismatch on sealing".into(),
                source,
            )];
        }

        if sealed_blob.hash != measurement {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Sealed,
                "sealed blob hash does not match declared measurement".into(),
                source,
            )];
        }

        if let Some(blob) = inline_blob {
            let mut hasher = Hasher::new();
            hasher.update(blob);
            let digest = *hasher.finalize().as_bytes();
            if digest != measurement {
                return vec![self.error_event(
                    muscle,
                    LifecycleStage::Sealed,
                    "inline blob hash mismatch".into(),
                    source,
                )];
            }
            self.store.put_with_digest(digest, blob.clone());
        }

        let attestation_hash = match self.find_valid_attestation(&source.attestations, measurement)
        {
            Ok(hash) => hash,
            Err(reason) => {
                return vec![self.error_event(muscle, LifecycleStage::Sealed, reason, source)]
            }
        };

        record.sealed_blob = Some(sealed_blob.clone());
        record.attestation = Some(attestation_hash);
        record.stage = LifecycleStage::Sealed;
        record.last_error = None;

        vec![self.lifecycle_event(
            MuscleEvent::LifecycleUpdate(LifecycleUpdate::Sealed {
                muscle: muscle.clone(),
                sealed_blob: sealed_blob.clone(),
                attestation: attestation_hash,
            }),
            vec![sealed_blob.clone()],
            source,
            DataSensitivity::Confidential,
        )]
    }

    fn handle_activate(
        &mut self,
        muscle: &MuscleRef,
        policy: Option<ContentRef>,
        policy_tags: &[String],
        source: &LedgerEvent,
    ) -> Vec<LedgerEvent> {
        let Some(record) = self.registry.get_mut(&Self::key(muscle)) else {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Registered,
                "muscle not registered".into(),
                source,
            )];
        };

        if record.stage == LifecycleStage::Retired {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Retired,
                "retired muscle cannot be reactivated".into(),
                source,
            )];
        }

        if record.sealed_blob.is_none() || record.attestation.is_none() {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Sealed,
                "sealing prerequisites missing".into(),
                source,
            )];
        }

        record.stage = LifecycleStage::Active;
        record.policy = policy.clone();
        if !policy_tags.is_empty() {
            record.policy_tags = policy_tags.to_vec();
        }
        record.last_error = None;

        let mut attachments = Vec::new();
        if let Some(policy_ref) = &policy {
            attachments.push(policy_ref.clone());
        }

        vec![self.lifecycle_event(
            MuscleEvent::LifecycleUpdate(LifecycleUpdate::Activated {
                muscle: muscle.clone(),
                policy,
                policy_tags: record.policy_tags.clone(),
            }),
            attachments,
            source,
            DataSensitivity::Internal,
        )]
    }

    fn handle_retire(
        &mut self,
        muscle: &MuscleRef,
        reason: &str,
        source: &LedgerEvent,
    ) -> Vec<LedgerEvent> {
        let key = Self::key(muscle);
        if let Some(record) = self.registry.get_mut(&key) {
            record.stage = LifecycleStage::Retired;
            record.last_error = None;
        } else {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Registered,
                "cannot retire unknown muscle".into(),
                source,
            )];
        }

        vec![self.lifecycle_event(
            MuscleEvent::LifecycleUpdate(LifecycleUpdate::Retired {
                muscle: muscle.clone(),
                reason: reason.to_string(),
            }),
            Vec::new(),
            source,
            DataSensitivity::Internal,
        )]
    }

    fn validate_invocation(
        &mut self,
        muscle: &MuscleRef,
        policy: Option<&ContentRef>,
        request_tags: &[String],
        source: &LedgerEvent,
    ) -> Vec<LedgerEvent> {
        let Some(record) = self.registry.get_mut(&Self::key(muscle)) else {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Registered,
                "invocation for unknown muscle".into(),
                source,
            )];
        };

        if record.stage != LifecycleStage::Active {
            return vec![self.error_event(
                muscle,
                record.stage,
                "muscle not active for invocation".into(),
                source,
            )];
        }

        if record.attestation.is_none() || record.sealed_blob.is_none() {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Sealed,
                "invocation rejected: missing attestation or sealed blob".into(),
                source,
            )];
        }

        if let Some(expected_policy) = &record.policy {
            if policy.map(|p| &p.hash) != Some(&expected_policy.hash) {
                return vec![self.error_event(
                    muscle,
                    LifecycleStage::Active,
                    "invocation policy mismatch".into(),
                    source,
                )];
            }
        } else if policy.is_some() && record.policy.is_none() {
            // Allow invoker to provide stricter policy but ensure tags match required set.
        } else if record.policy.is_some() && policy.is_none() {
            return vec![self.error_event(
                muscle,
                LifecycleStage::Active,
                "invocation missing required policy reference".into(),
                source,
            )];
        }

        if !record.policy_tags.is_empty() {
            let provided: HashSet<&str> = request_tags.iter().map(String::as_str).collect();
            let required: HashSet<&str> = record.policy_tags.iter().map(String::as_str).collect();
            if !required.is_subset(&provided) {
                return vec![self.error_event(
                    muscle,
                    LifecycleStage::Active,
                    format!(
                        "missing policy tags: {:?}",
                        required.difference(&provided).cloned().collect::<Vec<_>>()
                    ),
                    source,
                )];
            }
        }

        Vec::new()
    }

    fn lifecycle_event(
        &self,
        muscle_event: MuscleEvent,
        attachments: Vec<ContentRef>,
        source: &LedgerEvent,
        sensitivity: DataSensitivity,
    ) -> LedgerEvent {
        LedgerEvent::new(
            EventKind::Muscle(muscle_event),
            self.issuer,
            Audience::Broadcast,
            source.created_at,
            sensitivity,
            attachments,
            Some(source.id),
        )
        .expect("lifecycle events should serialize")
    }

    fn error_event(
        &self,
        muscle: &MuscleRef,
        stage: LifecycleStage,
        reason: String,
        source: &LedgerEvent,
    ) -> LedgerEvent {
        self.lifecycle_event(
            MuscleEvent::LifecycleError(LifecycleError {
                muscle: muscle.clone(),
                stage,
                reason,
            }),
            Vec::new(),
            source,
            DataSensitivity::Internal,
        )
    }

    fn find_valid_attestation(
        &self,
        attestations: &[Attestation],
        measurement: Hash,
    ) -> Result<Hash, String> {
        for att in attestations {
            let AttestationKind::Build {
                artifact_hash,
                builder: _,
            } = &att.statement
            else {
                continue;
            };
            if artifact_hash != &measurement {
                continue;
            }
            let expected_hash = hash_attestation_statement(&att.statement);
            if expected_hash != att.statement_hash {
                return Err("attestation statement hash mismatch".into());
            }
            let pk = ed25519_dalek::VerifyingKey::from_bytes(&att.issuer)
                .map_err(|_| "invalid attestation issuer key")?;
            let sig = ed25519_dalek::Signature::from_bytes(&att.signature);
            pk.verify_strict(&att.statement_hash, &sig)
                .map_err(|_| "attestation signature invalid")?;
            return Ok(att.statement_hash);
        }
        Err("no matching build attestation for measurement".into())
    }

    fn key(muscle: &MuscleRef) -> MuscleKey {
        (muscle.id, muscle.version)
    }
}

fn fmt_hash(hash: &Hash) -> String {
    blake3::Hash::from_bytes(*hash).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use rand_core::OsRng;

    fn signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    fn sample_muscle() -> MuscleRef {
        MuscleRef {
            id: [0xAA; 32],
            version: 1,
        }
    }

    fn measurement_and_blob() -> (Hash, Vec<u8>, ContentRef) {
        let blob = b"sealed-muscle-blob".to_vec();
        let mut hasher = Hasher::new();
        hasher.update(&blob);
        let digest = *hasher.finalize().as_bytes();
        let cref = ContentRef {
            locator: "cas:sealed".into(),
            hash: digest,
            media_type: Some("application/octet-stream".into()),
            bytes: Some(blob.len() as u64),
        };
        (digest, blob, cref)
    }

    fn lifecycle_manager() -> (MuscleLifecycleManager, SigningKey) {
        let sk = signing_key();
        let manager = MuscleLifecycleManager::new(
            ContentStore::default(),
            sk.verifying_key().to_bytes(),
            "muscle.lifecycle",
            1,
        );
        (manager, sk)
    }

    fn ledger_event(kind: EventKind, parent: Option<ledger_spec::events::EventId>) -> LedgerEvent {
        let issuer = signing_key();
        LedgerEvent::new(
            kind,
            issuer.verifying_key().to_bytes(),
            Audience::Broadcast,
            1,
            DataSensitivity::Internal,
            Vec::new(),
            parent,
        )
        .unwrap()
    }

    fn build_attestation(measurement: Hash, sk: &SigningKey) -> Attestation {
        let statement = AttestationKind::Build {
            artifact_hash: measurement,
            builder: "compiler".into(),
        };
        let statement_hash = hash_attestation_statement(&statement);
        let signature = sk.sign(&statement_hash).to_bytes();
        Attestation {
            issuer: sk.verifying_key().to_bytes(),
            statement,
            statement_hash,
            signature,
        }
    }

    #[test]
    fn register_to_activate_to_invoke() {
        let (mut manager, sk) = lifecycle_manager();
        let muscle = sample_muscle();
        let (measurement, blob_bytes, sealed_ref) = measurement_and_blob();
        let register_event = ledger_event(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Register {
                muscle: muscle.clone(),
                measurement,
                manifest: None,
                policy_tags: vec!["safety".into()],
            })),
            None,
        );
        let register_outcomes = manager.handle_event(&register_event);
        assert_eq!(register_outcomes.len(), 1);

        let mut seal_event = ledger_event(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Seal {
                muscle: muscle.clone(),
                sealed_blob: sealed_ref.clone(),
                measurement,
                inline_blob: Some(blob_bytes.clone()),
            })),
            Some(register_outcomes[0].id),
        );
        seal_event
            .attestations
            .push(build_attestation(measurement, &sk));
        let seal_outcomes = manager.handle_event(&seal_event);
        assert_eq!(seal_outcomes.len(), 1);

        let activate_event = ledger_event(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Activate {
                muscle: muscle.clone(),
                policy: None,
                policy_tags: vec!["safety".into()],
            })),
            Some(seal_outcomes[0].id),
        );
        let activation = manager.handle_event(&activate_event);
        assert_eq!(activation.len(), 1);

        let invoke_event = ledger_event(
            EventKind::Muscle(MuscleEvent::InvocationRequest {
                muscle: muscle.clone(),
                input: ContentRef {
                    locator: "cas:input".into(),
                    hash: [0x11; 32],
                    media_type: None,
                    bytes: Some(16),
                },
                policy: None,
                policy_tags: vec!["safety".into()],
                return_channel: "results".into(),
                deterministic: true,
            }),
            activation.last().map(|e| e.id),
        );
        let violations = manager.handle_event(&invoke_event);
        assert!(violations.is_empty());
    }

    #[test]
    fn rejects_invocation_without_policy_tags() {
        let (mut manager, sk) = lifecycle_manager();
        let muscle = sample_muscle();
        let (measurement, blob_bytes, sealed_ref) = measurement_and_blob();

        let register_event = ledger_event(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Register {
                muscle: muscle.clone(),
                measurement,
                manifest: None,
                policy_tags: vec!["p1".into()],
            })),
            None,
        );
        let register_outcomes = manager.handle_event(&register_event);

        let mut seal_event = ledger_event(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Seal {
                muscle: muscle.clone(),
                sealed_blob: sealed_ref.clone(),
                measurement,
                inline_blob: Some(blob_bytes),
            })),
            Some(register_outcomes[0].id),
        );
        seal_event
            .attestations
            .push(build_attestation(measurement, &sk));
        let seal_outcomes = manager.handle_event(&seal_event);

        let activate_event = ledger_event(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Activate {
                muscle: muscle.clone(),
                policy: None,
                policy_tags: vec!["p1".into()],
            })),
            Some(seal_outcomes[0].id),
        );
        let _ = manager.handle_event(&activate_event);

        let invoke_event = ledger_event(
            EventKind::Muscle(MuscleEvent::InvocationRequest {
                muscle: muscle.clone(),
                input: ContentRef {
                    locator: "cas:input".into(),
                    hash: [0x22; 32],
                    media_type: None,
                    bytes: Some(8),
                },
                policy: None,
                policy_tags: vec![],
                return_channel: "results".into(),
                deterministic: false,
            }),
            None,
        );
        let violations = manager.handle_event(&invoke_event);
        assert_eq!(violations.len(), 1);
        match &violations[0].kind {
            EventKind::Muscle(MuscleEvent::LifecycleError(err)) => {
                assert_eq!(err.muscle, muscle);
                assert_eq!(err.stage, LifecycleStage::Active);
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
