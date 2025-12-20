//! Event and workflow schema layered on top of the ledger envelopes.
//!
//! The goal is to provide a typed event surface that all modules—Eä OS
//! brainstem, sealed muscles, companion Arda layer, and external auditors—can
//! share without coupling to a specific transport or deployment topology.
//!
//! Each `LedgerEvent` is encoded as a JSON payload inside the canonical
//! `EnvelopeBody` with a payload type tag of `"ea.event.v1"`. The helper
//! functions in this module derive deterministic event identifiers, enforce the
//! payload tag, and round-trip between typed events and ledger envelopes.

use crate::{
    hash_body, Channel, Envelope, EnvelopeBody, EnvelopeHeader, Hash, PublicKey, SchemaVersion,
    Timestamp,
};
use blake3::Hasher;
use serde::{de::Error as DeError, Deserialize, Serialize};

/// Payload type tag for typed events carried inside envelope bodies.
pub const EVENT_PAYLOAD_TYPE: &str = "ea.event.v1";

/// Deterministic identifier for a ledger event (BLAKE3 hash).
pub type EventId = Hash;

/// How the event should be interpreted by recipients.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventIntent {
    /// Request that expects a response on the same correlation chain.
    Request,
    /// Response to a prior request.
    Response,
    /// One-way notification or observation.
    Notify,
}

/// Data classification for routing and policy enforcement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DataSensitivity {
    /// Publicly observable; safe for broadcast.
    Public,
    /// Internal but non-sensitive.
    Internal,
    /// Confidential and access-controlled.
    Confidential,
    /// Restricted to explicitly attested recipients.
    Restricted,
}

/// Audience routing hints. Policies still gate actual delivery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Audience {
    /// Broadcast to the channel.
    Broadcast,
    /// Direct message to specific principals.
    Principals(Vec<PublicKey>),
    /// Forward to a named companion domain (e.g., Arda UI runtime).
    Domain(String),
}

/// Content reference for detached blobs or opaque attachments.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentRef {
    /// Canonical locator (URI or content-addressed ref).
    pub locator: String,
    /// Integrity hash of referenced content.
    pub hash: Hash,
    /// Optional media type hint.
    pub media_type: Option<String>,
    /// Optional size for boundary checks.
    pub bytes: Option<u64>,
}

/// Representation of a muscle or model version.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MuscleRef {
    /// Logical muscle identifier.
    pub id: Hash,
    /// Version number (sealed blob version).
    pub version: u64,
}

/// Lifecycle stage of a muscle artifact.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleStage {
    /// Muscle has been registered with expected measurement.
    Registered,
    /// Sealed blob and attestation were accepted.
    Sealed,
    /// Muscle is eligible for invocation under policy constraints.
    Active,
    /// Muscle was retired and must not be executed.
    Retired,
}

/// Lifecycle commands emitted on the ledger to drive state transitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleCommand {
    /// Register a new muscle measurement and policy tags.
    Register {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Expected measurement hash for the muscle artifacts.
        measurement: Hash,
        /// Optional manifest or metadata reference.
        manifest: Option<ContentRef>,
        /// Policy tags that must be honored by invocations.
        #[serde(default)]
        policy_tags: Vec<String>,
    },
    /// Submit a sealed blob and accompanying attestation.
    Seal {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Sealed blob reference (CAS locator + hash).
        sealed_blob: ContentRef,
        /// Measurement hash that the blob must match.
        measurement: Hash,
        /// Optional inline sealed blob bytes for CAS hydration.
        #[serde(default)]
        inline_blob: Option<Vec<u8>>,
    },
    /// Activate a muscle after policy confirmation.
    Activate {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Policy bundle applied at activation.
        policy: Option<ContentRef>,
        /// Tags applied to the active policy bundle.
        #[serde(default)]
        policy_tags: Vec<String>,
    },
    /// Retire an existing muscle.
    Retire {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Human-readable retirement reason.
        reason: String,
    },
}

/// Lifecycle update emitted after processing commands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleUpdate {
    /// Registration accepted.
    Registered {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Measurement recorded.
        measurement: Hash,
        /// Policy tags bound to the registration.
        #[serde(default)]
        policy_tags: Vec<String>,
    },
    /// Sealing accepted.
    Sealed {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Blob reference stored in CAS.
        sealed_blob: ContentRef,
        /// Attestation statement hash that was validated.
        attestation: Hash,
    },
    /// Activation successful.
    Activated {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Policy reference now enforced.
        policy: Option<ContentRef>,
        /// Active policy tags.
        #[serde(default)]
        policy_tags: Vec<String>,
    },
    /// Retirement recorded.
    Retired {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Reason for retirement.
        reason: String,
    },
}

/// Lifecycle error emitted when a command cannot be honored.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LifecycleError {
    /// Target muscle reference.
    pub muscle: MuscleRef,
    /// Stage where the error occurred.
    pub stage: LifecycleStage,
    /// Error details.
    pub reason: String,
}

/// Control-plane messages governing channels, attestations, and policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ControlEvent {
    /// Advertise or update a channel specification.
    ChannelAnnouncement {
        /// Name of the channel being announced.
        channel: Channel,
        /// Policy hash for audit and replay validation.
        policy_hash: Hash,
        /// Optional human-readable summary.
        summary: Option<String>,
    },
    /// Publish an attestation digest (build/runtime/policy bundle).
    AttestationNotice {
        /// Hash of the attestation payload stored off-ledger.
        attestation_hash: Hash,
        /// Domain-separated label of the attestation.
        label: String,
    },
    /// Register a workflow contract that companions can subscribe to.
    WorkflowContract {
        /// Logical workflow name.
        name: String,
        /// Version of the workflow contract.
        version: u16,
        /// Hash of the contract document for determinism.
        contract_hash: Hash,
    },
}

/// Muscle execution intents and telemetry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MuscleEvent {
    /// Request to run a sealed muscle blob.
    InvocationRequest {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Content reference to sealed input bundle.
        input: ContentRef,
        /// Optional policy or recipe reference to pin behavior.
        policy: Option<ContentRef>,
        /// Tags describing which policy bundle must be enforced.
        #[serde(default)]
        policy_tags: Vec<String>,
        /// Channel where the result must be posted.
        return_channel: Channel,
        /// Whether deterministic replay is required for audit.
        deterministic: bool,
    },
    /// Result emitted by the muscle execution path.
    InvocationResult {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Output content reference.
        output: ContentRef,
        /// Execution metrics for audit.
        metrics: ExecutionMetrics,
        /// Optional chained evidence (e.g., SNARK, TEE report).
        evidence: Option<ContentRef>,
    },
    /// Runtime measurements emitted mid-flight.
    Telemetry {
        /// Target muscle reference.
        muscle: MuscleRef,
        /// Structured metrics payload.
        metrics: ExecutionMetrics,
    },
    /// Lifecycle commands and notifications.
    LifecycleCommand(LifecycleCommand),
    /// Lifecycle state updates.
    LifecycleUpdate(LifecycleUpdate),
    /// Lifecycle enforcement errors.
    LifecycleError(LifecycleError),
}

/// Observability and performance counters for muscle runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ExecutionMetrics {
    /// Total execution time in milliseconds.
    pub exec_ms: u64,
    /// CPU time in microseconds.
    pub cpu_us: u64,
    /// Peak memory bytes.
    pub peak_mem_bytes: u64,
    /// Optional energy consumption in microjoules.
    pub energy_uj: Option<u64>,
}

/// Ledger-driven audit events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuditEvent {
    /// Query a bounded window of envelopes.
    LogQuery {
        /// Channel being queried.
        channel: Channel,
        /// Inclusive starting offset.
        from: usize,
        /// Max number of entries to return.
        limit: usize,
    },
    /// Delivery of query results (hash-only or full records).
    LogResult {
        /// Channel that was queried.
        channel: Channel,
        /// Returned envelopes encoded as content reference.
        records: ContentRef,
    },
    /// Export request for compliant audit bundle.
    ExportRequest {
        /// Channel scope for export.
        channel: Channel,
        /// Hash of the export policy.
        policy_hash: Hash,
        /// Return channel for the export artifact.
        return_channel: Channel,
    },
    /// Export materialization complete.
    ExportReady {
        /// Export artifact reference.
        artifact: ContentRef,
    },
}

/// Privacy scanning workflow events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PrivacyEvent {
    /// Request to scan a document or message bundle.
    ScanRequested {
        /// Reference to the submitted content.
        document: ContentRef,
        /// Reference to the applied policy set.
        policy: ContentRef,
        /// Return channel for findings.
        return_channel: Channel,
    },
    /// Findings produced by the privacy muscle.
    FindingsReady {
        /// Reference to findings (redactions, scores).
        findings: ContentRef,
        /// Severity summary for routing.
        severity: PrivacySeverity,
    },
    /// Enforcement action applied to the content.
    ActionApplied {
        /// Action kind (redact, block, alert).
        action: PrivacyAction,
        /// Target content reference after enforcement.
        target: ContentRef,
    },
}

/// Severity gradation for privacy findings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PrivacySeverity {
    /// Informational only.
    Info,
    /// Needs review but not blocking.
    Review,
    /// High-risk content.
    High,
    /// Critical/stop-the-line.
    Critical,
}

/// Supported privacy enforcement actions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PrivacyAction {
    /// Redact sensitive spans.
    Redact,
    /// Block transmission.
    Block,
    /// Notify a specific channel.
    Alert(Channel),
    /// Escalate to a principal list.
    Escalate(Vec<PublicKey>),
}

/// User companion and UI-driven events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgencyEvent {
    /// Ledger-triggered browser fetch and summarize.
    BrowserFetch {
        /// Target URL.
        url: String,
        /// Hash of the retrieval policy.
        policy_hash: Hash,
        /// Return channel for the fetched content summary.
        return_channel: Channel,
    },
    /// Result of a browser fetch.
    BrowserResult {
        /// Content snapshot reference.
        content: ContentRef,
        /// Optional privacy scan reference applied to the snapshot.
        privacy: Option<ContentRef>,
    },
    /// Secure terminal command invocation.
    TerminalCommand {
        /// Command string (policy-validated).
        command: String,
        /// Return channel for command output.
        return_channel: Channel,
    },
    /// Terminal output and audit trail reference.
    TerminalResult {
        /// Output log reference.
        output: ContentRef,
        /// Non-zero if exit code indicates failure.
        exit_code: i32,
    },
    /// LLM/LORA fetch and sealing.
    ModelLoad {
        /// Model locator (e.g., HuggingFace path).
        model_ref: String,
        /// Compiled muscle reference once sealed.
        sealed_muscle: MuscleRef,
        /// Artifact bundle hash for attestation.
        artifact_hash: Hash,
    },
}

/// Events that flow through the ledger bus.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "data")]
pub enum EventKind {
    /// Control-plane message.
    Control(ControlEvent),
    /// Muscle execution path.
    Muscle(MuscleEvent),
    /// Audit queries and exports.
    Audit(AuditEvent),
    /// Privacy workflows.
    Privacy(PrivacyEvent),
    /// User companion/agency flows.
    Agency(AgencyEvent),
}

impl EventKind {
    /// Infer intent from the event kind for routing.
    pub fn intent(&self) -> EventIntent {
        match self {
            EventKind::Control(ControlEvent::ChannelAnnouncement { .. })
            | EventKind::Control(ControlEvent::AttestationNotice { .. })
            | EventKind::Control(ControlEvent::WorkflowContract { .. }) => EventIntent::Notify,
            EventKind::Muscle(MuscleEvent::InvocationRequest { .. })
            | EventKind::Audit(AuditEvent::LogQuery { .. })
            | EventKind::Audit(AuditEvent::ExportRequest { .. })
            | EventKind::Privacy(PrivacyEvent::ScanRequested { .. })
            | EventKind::Agency(AgencyEvent::BrowserFetch { .. })
            | EventKind::Agency(AgencyEvent::TerminalCommand { .. })
            | EventKind::Muscle(MuscleEvent::LifecycleCommand(_)) => EventIntent::Request,
            EventKind::Muscle(MuscleEvent::InvocationResult { .. })
            | EventKind::Muscle(MuscleEvent::Telemetry { .. })
            | EventKind::Audit(AuditEvent::LogResult { .. })
            | EventKind::Audit(AuditEvent::ExportReady { .. })
            | EventKind::Privacy(PrivacyEvent::FindingsReady { .. })
            | EventKind::Privacy(PrivacyEvent::ActionApplied { .. })
            | EventKind::Agency(AgencyEvent::BrowserResult { .. })
            | EventKind::Agency(AgencyEvent::TerminalResult { .. })
            | EventKind::Agency(AgencyEvent::ModelLoad { .. })
            | EventKind::Muscle(MuscleEvent::LifecycleUpdate(_))
            | EventKind::Muscle(MuscleEvent::LifecycleError(_)) => EventIntent::Response,
        }
    }
}

/// Typed ledger event with routing and classification metadata.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LedgerEvent {
    /// Deterministic event identifier.
    pub id: EventId,
    /// Optional parent correlation id.
    pub parent: Option<EventId>,
    /// Issuer identity (public key or domain key).
    pub issuer: PublicKey,
    /// Intended audience.
    pub audience: Audience,
    /// Creation timestamp (mirrors envelope timestamp).
    pub created_at: Timestamp,
    /// Declared data classification.
    pub sensitivity: DataSensitivity,
    /// Inferred or declared intent for routing.
    pub intent: EventIntent,
    /// Optional attachments stored off-ledger.
    pub attachments: Vec<ContentRef>,
    /// Domain-specific payload.
    pub kind: EventKind,
}

impl LedgerEvent {
    /// Build a new event and derive a deterministic identifier.
    pub fn new(
        kind: EventKind,
        issuer: PublicKey,
        audience: Audience,
        created_at: Timestamp,
        sensitivity: DataSensitivity,
        attachments: Vec<ContentRef>,
        parent: Option<EventId>,
    ) -> Result<Self, serde_json::Error> {
        let intent = kind.intent();
        let id = compute_event_id(&kind, created_at, &issuer, parent.as_ref())?;
        Ok(Self {
            id,
            parent,
            issuer,
            audience,
            created_at,
            sensitivity,
            intent,
            attachments,
            kind,
        })
    }

    /// Convert the event into a ledger envelope with the prescribed payload tag.
    pub fn into_envelope(
        self,
        channel: Channel,
        version: SchemaVersion,
    ) -> Result<Envelope, serde_json::Error> {
        let body = EnvelopeBody {
            payload: serde_json::to_value(&self)?,
            payload_type: Some(EVENT_PAYLOAD_TYPE.into()),
        };
        let body_hash = hash_body(&body);
        Ok(Envelope {
            header: EnvelopeHeader {
                channel,
                version,
                prev: None,
                body_hash,
                timestamp: self.created_at,
            },
            body,
            signatures: Vec::new(),
            attestations: Vec::new(),
        })
    }

    /// Decode a typed event from a ledger envelope, enforcing the payload tag.
    pub fn from_envelope(env: &Envelope) -> Result<Self, serde_json::Error> {
        let payload_type = env
            .body
            .payload_type
            .as_deref()
            .ok_or_else(|| DeError::custom("missing payload_type for event"))?;
        if payload_type != EVENT_PAYLOAD_TYPE {
            return Err(DeError::custom(format!(
                "unexpected payload_type: {payload_type}"
            )));
        }
        serde_json::from_value(env.body.payload.clone())
    }
}

/// Compute a deterministic event identifier from payload, issuer, and time.
pub fn compute_event_id(
    kind: &EventKind,
    created_at: Timestamp,
    issuer: &PublicKey,
    parent: Option<&EventId>,
) -> Result<EventId, serde_json::Error> {
    let mut hasher = Hasher::new();
    hasher.update(b"ea-ledger:event-id:v1");
    hasher.update(&created_at.to_le_bytes());
    hasher.update(issuer);
    if let Some(parent_id) = parent {
        hasher.update(parent_id);
    }
    let encoded = serde_json::to_vec(kind)?;
    hasher.update(&encoded);
    Ok(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;

    fn key() -> PublicKey {
        ed25519_dalek::SigningKey::generate(&mut OsRng)
            .verifying_key()
            .to_bytes()
    }

    fn sample_ref() -> ContentRef {
        ContentRef {
            locator: "content:sample".into(),
            hash: [0xAA; 32],
            media_type: Some("application/json".into()),
            bytes: Some(1024),
        }
    }

    #[test]
    fn roundtrip_event_envelope() {
        let issuer = key();
        let muscle = MuscleEvent::InvocationRequest {
            muscle: MuscleRef {
                id: [0x11; 32],
                version: 1,
            },
            input: sample_ref(),
            policy: None,
            policy_tags: Vec::new(),
            return_channel: "muscle.results".into(),
            deterministic: true,
        };
        let event = LedgerEvent::new(
            EventKind::Muscle(muscle),
            issuer,
            Audience::Domain("arda".into()),
            1,
            DataSensitivity::Confidential,
            Vec::new(),
            None,
        )
        .unwrap();

        let envelope = event.clone().into_envelope("muscle.io".into(), 1).unwrap();
        let restored = LedgerEvent::from_envelope(&envelope).unwrap();
        assert_eq!(event.id, restored.id);
        assert_eq!(event.intent, restored.intent);
        assert_eq!(event.kind, restored.kind);
    }

    #[test]
    fn compute_id_is_deterministic() {
        let issuer = key();
        let kind = EventKind::Audit(AuditEvent::LogQuery {
            channel: "audit.health".into(),
            from: 0,
            limit: 10,
        });
        let id1 = compute_event_id(&kind, 99, &issuer, None).unwrap();
        let id2 = compute_event_id(&kind, 99, &issuer, None).unwrap();
        assert_eq!(id1, id2);
    }
}
