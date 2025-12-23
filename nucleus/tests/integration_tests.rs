#![cfg(test)]

use nucleus::integration::{HardwareAttestation, LatticeStream};
use nucleus::kernel::MuscleNucleus;

#[test]
fn test_boot_rule_verification() {
    let mut attestation = HardwareAttestation::new();
    let lattice = LatticeStream::new();

    // Boot rule should pass with valid attestation
    assert!(attestation.verify());
    // Lattice root verification would depend on actual genesis
}

#[test]
fn test_nucleus_creation() {
    let nucleus = MuscleNucleus::new();

    // Verify fixed size
    assert_eq!(core::mem::size_of::<MuscleNucleus>(), 8192);

    // Verify capabilities are set
    assert!(nucleus.capabilities.can_load_muscle());
}
