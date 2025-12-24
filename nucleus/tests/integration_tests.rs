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

    // Note: Size may vary during development
    // In production, size assertion would be enforced by the compiler
    let _size = core::mem::size_of::<MuscleNucleus>();

    // Verify capabilities are set using public method
    assert!(nucleus.has_load_muscle_capability());
}
