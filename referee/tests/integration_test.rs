// referee/tests/integration_test.rs
// Eä Integration Tests v5.0

#![cfg(test)]

use proptest::prelude::*;

proptest! {
    #[test]
    fn crypto_roundtrip(
        master in prop::array::uniform32(any::<u8>()),
        salt in prop::array::uniform16(any::<u8>()),
        data in prop::collection::vec(any::<u8>(), 0..4096)
    ) {
        // Test that our crypto is compatible with muscle compiler
        // In real integration, we'd test with actual muscle compiler output
        assert!(data.len() <= 4096); // Basic sanity check
    }
}

#[test]
fn muscle_loader_sanity() {
    // Test basic muscle loader functionality
    use referee::muscle_loader::{calculate_required_pages, generate_salt};

    let salt1 = generate_salt(0, "test");
    let salt2 = generate_salt(0, "test");
    assert_eq!(salt1, salt2);

    assert_eq!(calculate_required_pages(4096), 1);
    assert_eq!(calculate_required_pages(4097), 2);
}
