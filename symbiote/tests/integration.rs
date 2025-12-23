use ea_lattice_ledger::{LatticeRoot, MuscleUpdate};
use ea_symbiote::{PolicyAction, PolicyEngine, Symbiote};

#[test]
fn test_symbiote_initialization() {
    let root = [0u8; 32];
    let symbiote = Symbiote::new(root);

    assert_eq!(symbiote.current_root, root);
    assert!(symbiote.policy_engine.policy_count() > 0);
}

#[test]
fn test_policy_evaluation() {
    let root = [0u8; 32];
    let symbiote = Symbiote::new(root);

    // Create update that matches default policy
    let update = MuscleUpdate {
        muscle_id: [0xEA; 32],
        version: 42,
        blob: [0u8; 8256],
        proof: [0u8; 48],
    };

    let action = symbiote.process_update(&update);
    assert!(action.is_some());

    if let Some(PolicyAction::HealVulnerability {
        muscle_id,
        vulnerable_version,
        ..
    }) = action
    {
        assert_eq!(muscle_id, [0xEA; 32]);
        assert_eq!(vulnerable_version, 42);
    } else {
        panic!("Expected healing action");
    }
}

#[test]
fn test_quarantine_functionality() {
    let root = [0u8; 32];
    let symbiote = Symbiote::new(root);

    // Test quarantine check
    assert!(!symbiote.should_quarantine([0x42; 32], 1));
}

#[test]
fn test_patch_management() {
    use ea_symbiote::patches::{get_patch, list_patches};

    let patches = list_patches();
    assert!(!patches.is_empty());

    let patch_id = blake3::hash(b"patch_cve_2026_01").as_bytes();
    let patch = get_patch(patch_id);
    assert!(patch.is_some());

    if let Some(p) = patch {
        assert!(!p.description().is_empty());
    }
}

#[test]
fn test_symbiote_config() {
    use ea_symbiote::SymbioteConfig;

    let config = SymbioteConfig::default();
    assert!(config.auto_heal);
    assert!(config.quarantine);
    assert_eq!(config.max_healing_attempts, 3);
}

// Property-based tests
proptest::proptest! {
    #[test]
    fn prop_symbiote_handles_any_update(
        root in proptest::array::uniform32(proptest::arbitrary::any::<u8>()),
        muscle_id in proptest::array::uniform32(proptest::arbitrary::any::<u8>()),
        version in 0u64..1000,
    ) {
        let symbiote = Symbiote::new(root);
        let update = MuscleUpdate {
            muscle_id,
            version,
            blob: [0u8; 8256],
            proof: [0u8; 48],
        };

        // Should not panic on any input
        let _ = symbiote.process_update(&update);
    }
}
