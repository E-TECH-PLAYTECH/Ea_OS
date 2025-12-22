//! Policy engine for security decision making

use ea_lattice_ledger::{MuscleUpdate, LatticeRoot};
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

/// Security policy action
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyAction {
    /// Heal a specific vulnerability
    HealVulnerability {
        /// Muscle identifier
        muscle_id: [u8; 32],
        /// Vulnerable version to patch
        vulnerable_version: u64,
        /// Patch identifier to apply
        patch_id: [u8; 32],
    },
    /// Quarantine a muscle
    QuarantineMuscle {
        /// Muscle identifier
        muscle_id: [u8; 32],
        /// Reason for quarantine
        reason: &'static str,
    },
}

/// Security policy definition
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    /// Policy identifier
    pub id: [u8; 32],
    /// Policy name
    pub name: &'static str,
    /// Muscle pattern to match
    pub muscle_pattern: Option<[u8; 32]>,
    /// Version range to match
    pub version_range: Option<(u64, u64)>,
    /// Action to take
    pub action: PolicyAction,
    /// Whether policy is enabled
    pub enabled: bool,
}

/// Policy engine for evaluating security policies
#[derive(Debug, Clone)]
pub struct PolicyEngine {
    /// Registered security policies
    policies: Vec<SecurityPolicy>,
    /// Quarantine list (muscle_id -> reason)
    quarantine_list: BTreeMap<[u8; 32], &'static str>,
    /// Healing attempts tracking
    healing_attempts: BTreeMap<[u8; 32], u32>,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        let mut engine = Self {
            policies: Vec::new(),
            quarantine_list: BTreeMap::new(),
            healing_attempts: BTreeMap::new(),
        };
        
        // Register default policies
        engine.register_default_policies();
        engine
    }
}

impl PolicyEngine {
    /// Evaluate an update against security policies
    pub fn evaluate(&self, update: &MuscleUpdate) -> Option<PolicyAction> {
        for policy in &self.policies {
            if !policy.enabled {
                continue;
            }

            if self.matches_policy(update, policy) {
                return Some(policy.action.clone());
            }
        }
        
        None
    }

    /// Check if a muscle should be quarantined
    pub fn should_quarantine(&self, muscle_id: [u8; 32], _version: u64) -> bool {
        self.quarantine_list.contains_key(&muscle_id)
    }

    /// Register a new security policy
    pub fn register_policy(&mut self, policy: SecurityPolicy) {
        self.policies.push(policy);
    }

    /// Add muscle to quarantine list
    pub fn quarantine_muscle(&mut self, muscle_id: [u8; 32], reason: &'static str) {
        self.quarantine_list.insert(muscle_id, reason);
    }

    /// Get number of active policies
    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    /// Check if update matches policy criteria
    fn matches_policy(&self, update: &MuscleUpdate, policy: &SecurityPolicy) -> bool {
        // Check muscle pattern
        if let Some(pattern) = policy.muscle_pattern {
            if update.muscle_id != pattern {
                return false;
            }
        }

        // Check version range
        if let Some((min, max)) = policy.version_range {
            if update.version < min || update.version > max {
                return false;
            }
        }

        true
    }

    /// Register default security policies
    fn register_default_policies(&mut self) {
        // Example policy: Heal known vulnerability in specific muscle version
        let heal_policy = SecurityPolicy {
            id: blake3::hash(b"heal_cve_2026_01").into(),
            name: "Heal CVE-2026-01 in muscle 0xEA...",
            muscle_pattern: Some([0xEA; 32]), // Example muscle ID
            version_range: Some((42, 42)),    // Specific vulnerable version
            action: PolicyAction::HealVulnerability {
                muscle_id: [0xEA; 32],
                vulnerable_version: 42,
                patch_id: blake3::hash(b"patch_cve_2026_01").into(),
            },
            enabled: true,
        };

        // Example policy: Quarantine known malicious patterns
        let quarantine_policy = SecurityPolicy {
            id: blake3::hash(b"quarantine_parasite").into(),
            name: "Quarantine parasite muscles",
            muscle_pattern: None, // Match any muscle with "parasite" in name
            version_range: None,
            action: PolicyAction::QuarantineMuscle {
                muscle_id: [0; 32], // Will be filled at match time
                reason: "Contains parasite pattern",
            },
            enabled: true,
        };

        self.policies.push(heal_policy);
        self.policies.push(quarantine_policy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_matching() {
        let engine = PolicyEngine::default();
        let update = MuscleUpdate {
            muscle_id: [0xEA; 32],
            version: 42,
            blob: [0; 8256],
            proof: [0; 48],
        };

        let action = engine.evaluate(&update);
        assert!(action.is_some());
        
        if let Some(PolicyAction::HealVulnerability { muscle_id, vulnerable_version, .. }) = action {
            assert_eq!(muscle_id, [0xEA; 32]);
            assert_eq!(vulnerable_version, 42);
        } else {
            panic!("Expected HealVulnerability action");
        }
    }

    #[test]
    fn test_quarantine() {
        let mut engine = PolicyEngine::default();
        engine.quarantine_muscle([0x42; 32], "Test quarantine");
        
        assert!(engine.should_quarantine([0x42; 32], 1));
        assert!(!engine.should_quarantine([0x43; 32], 1));
    }
}
