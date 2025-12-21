//! Base application orchestrators built atop the ledger protocol.
//!
//! Each orchestrator emits typed [`ledger_spec::events::LedgerEvent`] instances,
//! signs them for non-repudiation, and pushes all artifacts through the
//! [`crate::brainstem::Ledger`] content-addressed store to guarantee
//! verifiable, hash-linked workflows.

use std::sync::Arc;

use blake3::Hash as BlakeHash;
use ed25519_dalek::SigningKey;
use ledger_spec::events::{
    AgencyEvent, Audience, AuditEvent, ContentRef, DataSensitivity, EventId, EventKind,
    LedgerEvent, LifecycleStage, MuscleEvent, PrivacyAction, PrivacyEvent,
};
use ledger_spec::{Attestation, Channel, Hash, PublicKey, SchemaVersion, Timestamp};

use crate::brainstem::{Alert, AppendReceipt, Ledger, SliceQuery};
use crate::lifecycle::MuscleLifecycleManager;
use crate::signing;
use crate::MerkleReceipt;

/// Errors returned by orchestrators.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Ledger append or query failed.
    #[error("ledger error: {0:?}")]
    Ledger(Alert),
    /// Event encoding failed.
    #[error("serialization error: {0}")]
    Encode(#[from] serde_json::Error),
    /// Binary serialization failed.
    #[error("binary serialization error: {0}")]
    BinaryEncode(#[from] Box<bincode::ErrorKind>),
    /// Lifecycle guard rejected the request.
    #[error("lifecycle violation: {0}")]
    Lifecycle(String),
}

/// Shared context for application orchestrators.
#[derive(Clone)]
pub struct AppContext {
    ledger: Ledger,
    signer: Arc<SigningKey>,
    channel: String,
    schema_version: SchemaVersion,
}

impl AppContext {
    /// Create a new application context.
    pub fn new(
        ledger: Ledger,
        signer: SigningKey,
        channel: impl Into<String>,
        schema_version: SchemaVersion,
    ) -> Self {
        Self {
            ledger,
            signer: Arc::new(signer),
            channel: channel.into(),
            schema_version,
        }
    }

    fn issuer(&self) -> PublicKey {
        self.signer.verifying_key().to_bytes()
    }

    fn append_event(&self, event: LedgerEvent) -> Result<AppendReceipt, AppError> {
        let mut env = event
            .clone()
            .into_envelope(self.channel.clone(), self.schema_version)?;
        env.header.prev = self.ledger.tail_hash();
        signing::sign_envelope(&mut env, &self.signer);
        if !event.attestations.is_empty() {
            env.attestations = event.attestations.clone();
        }
        self.ledger.append(env).map_err(AppError::Ledger)
    }

    fn store_bytes(
        &self,
        bytes: Vec<u8>,
        media_type: Option<String>,
        locator_hint: Option<String>,
    ) -> ContentRef {
        let digest = self.ledger.content_store().put(bytes.clone());
        let locator = locator_hint.unwrap_or_else(|| {
            let hex = BlakeHash::from_bytes(digest).to_hex();
            format!("cas:{hex}")
        });
        ContentRef {
            locator,
            hash: digest,
            media_type,
            bytes: Some(bytes.len() as u64),
        }
    }

    fn store_receipt(&self, receipt: &MerkleReceipt) -> Result<ContentRef, AppError> {
        let encoded = serde_json::to_vec(receipt)?;
        Ok(self.store_bytes(encoded, Some("application/json".into()), None))
    }
}

/// Audit Terminal orchestrator: inference requests, logging, and export with proofs.
pub struct AuditTerminal {
    ctx: AppContext,
}

impl AuditTerminal {
    /// Create a new Audit Terminal bound to an application context.
    pub fn new(ctx: AppContext) -> Self {
        Self { ctx }
    }

    /// Emit a deterministic inference request with justification and CAS-backed input.
    pub fn request_inference(
        &self,
        muscle: ledger_spec::events::MuscleRef,
        input: Vec<u8>,
        justification: String,
        return_channel: Channel,
    ) -> Result<(AppendReceipt, ContentRef), AppError> {
        let input_ref = self
            .ctx
            .store_bytes(input, Some("application/octet-stream".into()), None);
        let event = LedgerEvent::new(
            EventKind::Audit(AuditEvent::InferenceRequested {
                muscle,
                input: input_ref.clone(),
                justification,
                return_channel,
                requester: self.ctx.issuer(),
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Confidential,
            vec![input_ref.clone()],
            None,
        )?;
        let receipt = self.ctx.append_event(event)?;
        let proof = self.ctx.store_receipt(&receipt.merkle)?;
        Ok((receipt, proof))
    }

    /// Log an inference result with proof attachment.
    pub fn log_inference(
        &self,
        muscle: ledger_spec::events::MuscleRef,
        output: Vec<u8>,
        metrics: Option<ledger_spec::events::ExecutionMetrics>,
        request: Option<EventId>,
        proof: ContentRef,
    ) -> Result<AppendReceipt, AppError> {
        let output_ref =
            self.ctx
                .store_bytes(output, Some("application/octet-stream".into()), None);
        let event = LedgerEvent::new(
            EventKind::Audit(AuditEvent::InferenceLogged {
                muscle,
                output: output_ref.clone(),
                metrics,
                proof: Some(proof),
                request,
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Confidential,
            vec![output_ref],
            request,
        )?;
        self.ctx.append_event(event)
    }

    /// Materialize and emit an export bundle that includes Merkle proofs.
    pub fn export_with_proofs(
        &self,
        channel: Channel,
        from: usize,
        limit: usize,
        policy_hash: Hash,
        justification: String,
        return_channel: Channel,
    ) -> Result<AppendReceipt, AppError> {
        let request_event = LedgerEvent::new(
            EventKind::Audit(AuditEvent::ExportRequest {
                channel: channel.clone(),
                policy_hash,
                return_channel: return_channel.clone(),
                justification,
                requester: self.ctx.issuer(),
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            Vec::new(),
            None,
        )?;
        let req_id = request_event.id;
        let _ = self.ctx.append_event(request_event)?;

        let response = self
            .ctx
            .ledger
            .query(SliceQuery {
                from,
                limit,
                include_payloads: true,
            })
            .map_err(AppError::Ledger)?;

        let artifact_bytes = bincode::serialize(&response)?;
        let artifact = self.ctx.store_bytes(
            artifact_bytes,
            Some("application/octet-stream".into()),
            None,
        );
        let merkle_bundle = self.ctx.store_bytes(
            serde_json::to_vec(&response.receipts)?,
            Some("application/json".into()),
            None,
        );

        let ready_event = LedgerEvent::new(
            EventKind::Audit(AuditEvent::ExportReady {
                artifact: artifact.clone(),
                merkle_bundle: Some(merkle_bundle.clone()),
                request: Some(req_id),
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            vec![artifact, merkle_bundle],
            Some(req_id),
        )?;

        self.ctx.append_event(ready_event)
    }
}

/// Privacy Analyzer orchestrator: document submit → scan → findings → action.
pub struct PrivacyAnalyzer {
    ctx: AppContext,
}

impl PrivacyAnalyzer {
    /// Create a new Privacy Analyzer.
    pub fn new(ctx: AppContext) -> Self {
        Self { ctx }
    }

    /// Submit a document for scanning through a designated muscle.
    pub fn submit_document(
        &self,
        document: Vec<u8>,
        policy: ContentRef,
        return_channel: Channel,
        muscle: ledger_spec::events::MuscleRef,
        lifecycle: Option<&MuscleLifecycleManager>,
    ) -> Result<AppendReceipt, AppError> {
        Self::guard_muscle_active(lifecycle, &muscle)?;
        let doc_ref = self
            .ctx
            .store_bytes(document, Some("application/octet-stream".into()), None);
        let event = LedgerEvent::new(
            EventKind::Privacy(PrivacyEvent::ScanRequested {
                document: doc_ref.clone(),
                policy,
                return_channel,
                muscle: Some(muscle),
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Confidential,
            vec![doc_ref],
            None,
        )?;
        self.ctx.append_event(event)
    }

    /// Publish findings from a privacy scan.
    pub fn publish_findings(
        &self,
        findings: Vec<u8>,
        severity: ledger_spec::events::PrivacySeverity,
        muscle: ledger_spec::events::MuscleRef,
    ) -> Result<AppendReceipt, AppError> {
        let findings_ref = self
            .ctx
            .store_bytes(findings, Some("application/json".into()), None);
        let event = LedgerEvent::new(
            EventKind::Privacy(PrivacyEvent::FindingsReady {
                findings: findings_ref.clone(),
                severity,
                muscle: Some(muscle),
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            vec![findings_ref],
            None,
        )?;
        self.ctx.append_event(event)
    }

    /// Apply a downstream action such as redact/alert/report.
    pub fn apply_action(
        &self,
        action: PrivacyAction,
        target: Vec<u8>,
        muscle: ledger_spec::events::MuscleRef,
    ) -> Result<AppendReceipt, AppError> {
        let target_ref = self
            .ctx
            .store_bytes(target, Some("application/json".into()), None);
        let event = LedgerEvent::new(
            EventKind::Privacy(PrivacyEvent::ActionApplied {
                action,
                target: target_ref.clone(),
                muscle: Some(muscle),
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            vec![target_ref],
            None,
        )?;
        self.ctx.append_event(event)
    }

    fn guard_muscle_active(
        lifecycle: Option<&MuscleLifecycleManager>,
        muscle: &ledger_spec::events::MuscleRef,
    ) -> Result<(), AppError> {
        if let Some(manager) = lifecycle {
            let Some(record) = manager.record(muscle) else {
                return Err(AppError::Lifecycle(
                    "muscle not registered in lifecycle manager".into(),
                ));
            };
            if record.stage != LifecycleStage::Active {
                return Err(AppError::Lifecycle(format!(
                    "muscle not active (stage={:?})",
                    record.stage
                )));
            }
        }
        Ok(())
    }
}

/// Agency Assistant orchestrator: ledgered terminal, browser, and model lifecycle.
pub struct AgencyAssistant {
    ctx: AppContext,
}

impl AgencyAssistant {
    /// Create a new Agency Assistant.
    pub fn new(ctx: AppContext) -> Self {
        Self { ctx }
    }

    /// Issue a terminal command request with optional justification.
    pub fn terminal_command(
        &self,
        command: String,
        return_channel: Channel,
        justification: Option<String>,
    ) -> Result<AppendReceipt, AppError> {
        let event = LedgerEvent::new(
            EventKind::Agency(AgencyEvent::TerminalCommand {
                command,
                return_channel,
                justification,
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            Vec::new(),
            None,
        )?;
        self.ctx.append_event(event)
    }

    /// Persist terminal output and append a result event.
    pub fn terminal_result(
        &self,
        output: Vec<u8>,
        exit_code: i32,
    ) -> Result<AppendReceipt, AppError> {
        let output_ref = self
            .ctx
            .store_bytes(output, Some("text/plain".into()), None);
        let event = LedgerEvent::new(
            EventKind::Agency(AgencyEvent::TerminalResult {
                output: output_ref.clone(),
                exit_code,
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            vec![output_ref],
            None,
        )?;
        self.ctx.append_event(event)
    }

    /// Emit a browser fetch request.
    pub fn browser_fetch(
        &self,
        url: String,
        policy_hash: Hash,
        return_channel: Channel,
    ) -> Result<AppendReceipt, AppError> {
        let event = LedgerEvent::new(
            EventKind::Agency(AgencyEvent::BrowserFetch {
                url,
                policy_hash,
                return_channel,
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Confidential,
            Vec::new(),
            None,
        )?;
        self.ctx.append_event(event)
    }

    /// Publish browser fetch results with optional summary and privacy scan reference.
    pub fn browser_result(
        &self,
        content: Vec<u8>,
        summary: Option<Vec<u8>>,
        privacy: Option<ContentRef>,
    ) -> Result<AppendReceipt, AppError> {
        let content_ref =
            self.ctx
                .store_bytes(content, Some("application/octet-stream".into()), None);
        let summary_ref = summary.map(|bytes| {
            self.ctx
                .store_bytes(bytes, Some("text/markdown".into()), None)
        });
        let mut attachments = vec![content_ref.clone()];
        if let Some(ref summary) = summary_ref {
            attachments.push(summary.clone());
        }
        if let Some(ref privacy_ref) = privacy {
            attachments.push(privacy_ref.clone());
        }
        let event = LedgerEvent::new(
            EventKind::Agency(AgencyEvent::BrowserResult {
                content: content_ref,
                summary: summary_ref,
                privacy,
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Confidential,
            attachments,
            None,
        )?;
        self.ctx.append_event(event)
    }

    /// Record a model load workflow and attach attestation and registration channel.
    pub fn model_load(
        &self,
        model_ref: String,
        sealed_muscle: ledger_spec::events::MuscleRef,
        artifact: Vec<u8>,
        attestation_bytes: Vec<u8>,
        registry_channel: Channel,
    ) -> Result<AppendReceipt, AppError> {
        let artifact_digest = blake3::hash(&artifact);
        let artifact_ref =
            self.ctx
                .store_bytes(artifact, Some("application/octet-stream".into()), None);
        let attestation_ref =
            self.ctx
                .store_bytes(attestation_bytes, Some("application/json".into()), None);
        let event = LedgerEvent::new(
            EventKind::Agency(AgencyEvent::ModelLoad {
                model_ref,
                sealed_muscle,
                artifact_hash: *artifact_digest.as_bytes(),
                attestation: Some(attestation_ref.clone()),
                registry_channel,
            }),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            vec![artifact_ref, attestation_ref],
            None,
        )?;
        self.ctx.append_event(event)
    }

    /// Register and seal a model through lifecycle commands with an attestation attachment.
    pub fn register_and_seal_model(
        &self,
        muscle: ledger_spec::events::MuscleRef,
        measurement: Hash,
        manifest: Option<ContentRef>,
        sealed_blob: ContentRef,
        attestation: Attestation,
        policy_tags: Vec<String>,
    ) -> Result<(AppendReceipt, AppendReceipt), AppError> {
        let register_event = LedgerEvent::new(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(
                ledger_spec::events::LifecycleCommand::Register {
                    muscle: muscle.clone(),
                    measurement,
                    manifest: manifest.clone(),
                    policy_tags: policy_tags.clone(),
                },
            )),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            manifest.into_iter().collect::<Vec<_>>(),
            None,
        )?;
        let register_id = register_event.id;
        let register_receipt = self.ctx.append_event(register_event)?;

        let mut seal_event = LedgerEvent::new(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(
                ledger_spec::events::LifecycleCommand::Seal {
                    muscle: muscle.clone(),
                    sealed_blob: sealed_blob.clone(),
                    measurement,
                    inline_blob: None,
                },
            )),
            self.ctx.issuer(),
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Confidential,
            vec![sealed_blob],
            Some(register_id),
        )?;
        seal_event = seal_event.with_attestations(vec![attestation]);
        let seal_receipt = self.ctx.append_event(seal_event)?;

        Ok((register_receipt, seal_receipt))
    }
}

fn now_millis() -> Timestamp {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brainstem::ContentStore;
    use crate::lifecycle::MuscleLifecycleManager;
    use ledger_spec::events::LifecycleCommand;
    use ledger_spec::ChannelRegistry;
    use rand_core::OsRng;

    fn base_context() -> AppContext {
        let mut registry = ChannelRegistry::new();
        registry.upsert(ledger_spec::ChannelSpec {
            name: "apps.bus".into(),
            policy: ledger_spec::ChannelPolicy {
                min_signers: 1,
                allowed_signers: Vec::new(),
                require_attestations: false,
                enforce_timestamp_ordering: true,
            },
        });
        let ledger = Ledger::new(registry);
        let signer = SigningKey::generate(&mut OsRng);
        AppContext::new(ledger, signer, "apps.bus", 1)
    }

    #[test]
    fn export_includes_merkle_proofs() {
        let ctx = base_context();
        let audit = AuditTerminal::new(ctx.clone());
        let (req_receipt, proof_ref) = audit
            .request_inference(
                ledger_spec::events::MuscleRef {
                    id: [1u8; 32],
                    version: 1,
                },
                b"input".to_vec(),
                "test inference".into(),
                "results".into(),
            )
            .unwrap();
        assert_eq!(req_receipt.merkle.index, 0);
        assert!(proof_ref.bytes.is_some());

        let ready = audit
            .export_with_proofs(
                "apps.bus".into(),
                0,
                8,
                [9u8; 32],
                "export audit".into(),
                "exports".into(),
            )
            .unwrap();
        assert!(ready.merkle.verify());
    }

    #[test]
    fn privacy_analyzer_enforces_lifecycle_when_provided() {
        let ctx = base_context();
        let mut lifecycle = lifecycle_manager();
        let analyzer = PrivacyAnalyzer::new(ctx);
        let muscle = ledger_spec::events::MuscleRef {
            id: [3u8; 32],
            version: 1,
        };
        // Without active state this should fail.
        let err = analyzer
            .submit_document(
                b"doc".to_vec(),
                ContentRef {
                    locator: "policy".into(),
                    hash: [1u8; 32],
                    media_type: None,
                    bytes: Some(3),
                },
                "return".into(),
                muscle.clone(),
                Some(&lifecycle),
            )
            .unwrap_err();
        assert!(matches!(err, AppError::Lifecycle(_)));

        activate_muscle(&mut lifecycle, muscle.clone());
        analyzer
            .submit_document(
                b"doc".to_vec(),
                ContentRef {
                    locator: "policy".into(),
                    hash: [1u8; 32],
                    media_type: None,
                    bytes: Some(3),
                },
                "return".into(),
                muscle,
                Some(&lifecycle),
            )
            .unwrap();
    }

    fn lifecycle_manager() -> MuscleLifecycleManager {
        let signer = SigningKey::generate(&mut OsRng);
        MuscleLifecycleManager::new(
            ContentStore::default(),
            signer.verifying_key().to_bytes(),
            "muscle.lifecycle",
            1,
        )
    }

    fn activate_muscle(
        manager: &mut MuscleLifecycleManager,
        muscle: ledger_spec::events::MuscleRef,
    ) {
        let seal_blob = manager.ingest_blob(b"sealed".to_vec(), None, None);
        let measurement = seal_blob.hash;
        let register = LedgerEvent::new(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Register {
                muscle: muscle.clone(),
                measurement,
                manifest: None,
                policy_tags: Vec::new(),
            })),
            [0u8; 32],
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            Vec::new(),
            None,
        )
        .unwrap();
        let att_statement = ledger_spec::AttestationKind::Build {
            artifact_hash: measurement,
            builder: "tester".into(),
        };
        let statement_hash = ledger_spec::hash_attestation_statement(&att_statement);
        let mut attestation = Attestation {
            issuer: [0u8; 32],
            statement: att_statement,
            statement_hash,
            signature: [0u8; 64],
        };
        signing::sign_attestation(&mut attestation, &SigningKey::generate(&mut OsRng));
        let seal = LedgerEvent::new(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Seal {
                muscle: muscle.clone(),
                sealed_blob: seal_blob.clone(),
                measurement,
                inline_blob: None,
            })),
            [0u8; 32],
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Confidential,
            vec![seal_blob],
            Some(register.id),
        )
        .unwrap()
        .with_attestations(vec![attestation]);
        let activate = LedgerEvent::new(
            EventKind::Muscle(MuscleEvent::LifecycleCommand(LifecycleCommand::Activate {
                muscle,
                policy: None,
                policy_tags: Vec::new(),
            })),
            [0u8; 32],
            Audience::Broadcast,
            now_millis(),
            DataSensitivity::Internal,
            Vec::new(),
            None,
        )
        .unwrap();

        for evt in [register, seal, activate] {
            let updates = manager.handle_event(&evt);
            // Drive state machine to completion.
            for update in updates {
                let _ = manager.handle_event(&update);
            }
        }
    }
}
