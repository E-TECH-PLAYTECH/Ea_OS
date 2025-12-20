//! Declarative policy definitions, decisions, and alerts.
//!
//! The policy model is intentionally declarative: rules describe the shape of
//! events they apply to (scope, tags, event kinds, and origin allowlists) and
//! produce side-effect free effects (block, require justification, reroute, or
//! allow). Execution happens in `ledger_core::policy::PolicyEngine`.

use serde::{Deserialize, Serialize};

use crate::events::{DataSensitivity, EventIntent};
use crate::{Audience, EventId, PublicKey, Timestamp};

/// Identifier for a versioned policy bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PolicyId {
    /// Human readable policy name.
    pub name: String,
    /// Monotonic version for upgrades.
    pub version: u16,
}

/// Surface where a rule applies.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyScope {
    /// Mutating or request-like events.
    Command,
    /// Result or response events.
    Result,
    /// Alert or anomaly events.
    Alert,
    /// Apply regardless of shape.
    Any,
}

/// Effect emitted by a rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyEffect {
    /// Permit the event.
    Allow,
    /// Block the event outright.
    Block {
        /// Human-readable reason.
        reason: String,
    },
    /// Require operator justification before allowing.
    RequireJustification {
        /// Why a justification is needed.
        reason: String,
    },
    /// Modify routing before allowing.
    Reroute {
        /// New audience to route to.
        audience: Audience,
        /// Optional explanation.
        reason: Option<String>,
    },
}

/// Declarative rule describing when a policy should fire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyRule {
    /// Rule identifier within the policy.
    pub id: String,
    /// Scope where this rule applies.
    pub scope: PolicyScope,
    /// Rule description for audit.
    pub description: Option<String>,
    /// Match when any of these tags are present on the event.
    #[serde(default)]
    pub match_tags_any: Vec<String>,
    /// Do not match if any of these tags are present (absence enforcement).
    #[serde(default)]
    pub absent_tags: Vec<String>,
    /// Match against fully-qualified event kind labels (e.g., `Audit.ExportRequest`).
    #[serde(default)]
    pub match_event_kinds: Vec<String>,
    /// Restrict rule to specific issuers; empty means any issuer.
    #[serde(default)]
    pub allowed_origins: Vec<PublicKey>,
    /// Minimum sensitivity required to trigger the rule.
    pub min_sensitivity: Option<DataSensitivity>,
    /// Effect enforced when the rule matches.
    pub effect: PolicyEffect,
}

/// Versioned policy bundle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyDefinition {
    /// Policy identifier.
    pub id: PolicyId,
    /// Policy description for humans.
    pub description: String,
    /// Tags advertised by the policy bundle.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Rules contained in the bundle.
    pub rules: Vec<PolicyRule>,
}

/// Mapping of a rule and its effect as applied to a subject event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyBinding {
    /// Applied policy identifier.
    pub policy: PolicyId,
    /// Rule identifier within that policy.
    pub rule_id: Option<String>,
    /// Effect that was produced.
    pub effect: PolicyEffect,
}

/// Decision rendered by the policy engine for a specific event.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyDecision {
    /// Event being evaluated.
    pub subject: EventId,
    /// Surface classification.
    pub scope: PolicyScope,
    /// Applied bindings in evaluation order.
    pub bindings: Vec<PolicyBinding>,
    /// Final effect after combining all rules.
    pub final_effect: PolicyEffect,
    /// Tags observed on the subject.
    pub observed_tags: Vec<String>,
    /// Issuer of the subject event.
    pub origin: PublicKey,
    /// Optional routed audience when `final_effect` is a reroute.
    pub routed_audience: Option<Audience>,
    /// Optional justification supplied by an operator.
    pub justification: Option<String>,
    /// Decision timestamp (mirrors subject created_at).
    pub created_at: Timestamp,
}

/// Severity for policy alerts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyAlertSeverity {
    /// Informational notification.
    Info,
    /// Requires operator attention but not blocking.
    Warning,
    /// Critical/blocking condition.
    Critical,
}

/// Alert emitted as part of policy enforcement.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PolicyAlert {
    /// Event that triggered the alert.
    pub subject: EventId,
    /// Policy that produced the alert, if any.
    pub policy: Option<PolicyId>,
    /// Alert severity.
    pub severity: PolicyAlertSeverity,
    /// Human-readable message.
    pub message: String,
    /// When the alert was created.
    pub created_at: Timestamp,
}

impl From<EventIntent> for PolicyScope {
    fn from(intent: EventIntent) -> Self {
        match intent {
            EventIntent::Request => PolicyScope::Command,
            EventIntent::Response => PolicyScope::Result,
            EventIntent::Notify => PolicyScope::Alert,
        }
    }
}
