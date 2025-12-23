use ea_lattice_ledger::*;
use proptest::prelude::*;

#[test]
fn test_basic_update_cycle() {
    let root = [0u8; 32];
    let id = [0xEAu8; 32];
    let mut blob = [0u8; MAX_BLOB];
    blob[0] = 0x77; // Some data

    let update = generate_update(id, 1, blob, root);
    assert!(verify_update(root, &update));
}

#[test]
fn test_version_rollback_prevention() {
    let root = [0u8; 32];
    let id = [0x42u8; 32];
    let blob = [0x99u8; MAX_BLOB];

    let update_v2 = generate_update(id, 2, blob, root);
    let update_v1 = generate_update(id, 1, blob, root);

    // Both should verify individually
    assert!(verify_update(root, &update_v2));
    assert!(verify_update(root, &update_v1));

    // But they should be different due to version in commitment
    assert_ne!(update_v1.proof, update_v2.proof);
}

#[test]
fn test_different_muscles_different_proofs() {
    let root = [0u8; 32];
    let blob = [0x88u8; MAX_BLOB];

    let id1 = [0x01u8; 32];
    let id2 = [0x02u8; 32];

    let update1 = generate_update(id1, 1, blob, root);
    let update2 = generate_update(id2, 1, blob, root);

    assert!(verify_update(root, &update1));
    assert!(verify_update(root, &update2));
    assert_ne!(update1.proof, update2.proof);
}

#[test]
fn test_tampered_blob_rejected() {
    let root = [0u8; 32];
    let id = [0xEAu8; 32];
    let mut blob = [0u8; MAX_BLOB];
    blob[100] = 0x42;

    let update = generate_update(id, 1, blob, root);

    // Tamper with blob
    let mut tampered_update = update;
    tampered_update.blob[100] = 0x43;

    assert!(!verify_update(root, &tampered_update));
}

#[test]
fn test_tampered_proof_rejected() {
    let root = [0u8; 32];
    let id = [0xEAu8; 32];
    let blob = [0u8; MAX_BLOB];

    let update = generate_update(id, 1, blob, root);

    // Tamper with proof
    let mut tampered_update = update;
    tampered_update.proof[0] ^= 0x01;

    assert!(!verify_update(root, &tampered_update));
}

proptest! {
    #[test]
    fn prop_any_update_verifies(
        root in prop::array::uniform32(any::<u8>()),
        id in prop::array::uniform32(any::<u8>()),
        version in 0u64..1000,
        blob_data in prop::collection::vec(any::<u8>(), 0..MAX_BLOB)
    ) {
        let mut blob = [0u8; MAX_BLOB];
        let len = blob_data.len().min(MAX_BLOB);
        blob[..len].copy_from_slice(&blob_data[..len]);

        let update = generate_update(id, version, blob, root);
        assert!(verify_update(root, &update));
    }

    #[test]
    fn prop_different_roots_different_proofs(
        root1 in prop::array::uniform32(any::<u8>()),
        root2 in prop::array::uniform32(any::<u8>()),
        id in prop::array::uniform32(any::<u8>()),
    ) {
        prop_assume!(root1 != root2);

        let blob = [0u8; MAX_BLOB];
        let update1 = generate_update(id, 1, blob, root1);
        let update2 = generate_update(id, 1, blob, root2);

        assert_ne!(update1.proof, update2.proof);
    }
}
