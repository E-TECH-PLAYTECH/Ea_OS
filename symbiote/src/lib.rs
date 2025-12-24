//! # Eä Symbiote
//!
//! Cryptographic immune system for autonomous security response via lattice policies.
//!
//! ## Overview
//!
//! Symbiote implements policy-as-code for the Eä ecosystem, providing automated
//! response to known vulnerabilities while maintaining all cryptographic security
//! guarantees.
//!
//! ## Security Model
//!
//! - **No privilege escalation**: Uses only public lattice capabilities
//! - **Append-only operations**: Cannot modify existing versions
//! - **Node autonomy**: Updates can be rejected by any node
//! - **Full auditability**: All actions permanently recorded on lattice
//!
//! ## Example
//!
//! ```rust,no_run
//! use ea_symbiote::{Symbiote, PolicyEngine};
//! use ea_lattice_ledger::MuscleUpdate;
//!
//! let root = [0u8; 32];  // Current lattice root
//! let symbiote = Symbiote::new(root);
//!
//! // Process a lattice update
//! let update = MuscleUpdate {
//!     muscle_id: [0u8; 32],
//!     version: 1,
//!     blob: [0u8; 8256],
//!     proof: [0u8; 48],
//! };
//!
//! if let Some(action) = symbiote.process_update(&update) {
//!     symbiote.execute_policy_action(action);
//! }
//! ```

#![no_std]
#![cfg_attr(feature = "bench", feature(test))]
#![deny(missing_docs, unsafe_code)]
#![warn(clippy::all, clippy::pedantic)]

extern crate alloc;

use ea_lattice_ledger::{verify_update, LatticeRoot, MuscleUpdate};

mod policy_engine;
pub use policy_engine::{PolicyAction, PolicyEngine, SecurityPolicy};

pub mod patches;

/// Symbiote core - cryptographic immune system
#[derive(Debug, Clone)]
pub struct Symbiote {
    /// Current lattice root for verification
    pub current_root: LatticeRoot,
    /// Policy engine for security decisions
    pub policy_engine: PolicyEngine,
}

impl Symbiote {
    /// Create a new Symbiote instance
    pub fn new(current_root: LatticeRoot) -> Self {
        Self {
            current_root,
            policy_engine: PolicyEngine::default(),
        }
    }

    /// Process a lattice update and return any required actions
    pub fn process_update(&self, update: &MuscleUpdate) -> Option<PolicyAction> {
        // Verify the update is valid before processing
        if !verify_update(self.current_root, update) {
            return None;
        }

        // Evaluate against security policies
        self.policy_engine.evaluate(update)
    }

    /// Process update without verification (TEST ONLY - bypasses cryptographic checks)
    /// This should only be used in tests where constructing valid proofs is not feasible.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn process_update_unchecked(&self, update: &MuscleUpdate) -> Option<PolicyAction> {
        self.policy_engine.evaluate(update)
    }

    /// Execute a policy action (typically would emit to lattice)
    pub fn execute_policy_action(&self, action: PolicyAction) -> Option<MuscleUpdate> {
        match action {
            PolicyAction::HealVulnerability {
                muscle_id,
                vulnerable_version,
                patch_id,
            } => {
                // Look up the patch and apply it
                if let Some(patch) = patches::get_patch(&patch_id) {
                    self.generate_healing_update(muscle_id, vulnerable_version, patch)
                } else {
                    None
                }
            }
            PolicyAction::QuarantineMuscle { muscle_id, reason } => {
                log::warn!("Quarantining muscle {}: {}", hex::encode(muscle_id), reason);
                None // Quarantine is enforced by not scheduling
            }
        }
    }

    /// Generate a healing update for a vulnerable muscle
    fn generate_healing_update(
        &self,
        _muscle_id: [u8; 32],
        _vulnerable_version: u64,
        _patch: &dyn patches::SecurityPatch,
    ) -> Option<MuscleUpdate> {
        // In real implementation, this would:
        // 1. Fetch current muscle source via introspection capability
        // 2. Apply the security patch
        // 3. Recompile and seal the patched muscle
        // 4. Generate lattice update for version + 1

        // For now, return None as placeholder
        // Full implementation requires integration with muscle compiler
        None
    }

    /// Verify if a muscle should be quarantined
    pub fn should_quarantine(&self, muscle_id: [u8; 32], version: u64) -> bool {
        self.policy_engine.should_quarantine(muscle_id, version)
    }
}

/// Symbiote configuration
#[derive(Debug, Clone)]
pub struct SymbioteConfig {
    /// Whether to enable automatic healing
    pub auto_heal: bool,
    /// Whether to enable quarantine
    pub quarantine: bool,
    /// Maximum healing attempts per muscle
    pub max_healing_attempts: u32,
}

impl Default for SymbioteConfig {
    fn default() -> Self {
        Self {
            auto_heal: true,
            quarantine: true,
            max_healing_attempts: 3,
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for Symbiote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Symbiote(root: {}, policies: {})",
            hex::encode(self.current_root),
            self.policy_engine.policy_count()
        )
    }
}
