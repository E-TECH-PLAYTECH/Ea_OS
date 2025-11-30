#![cfg(test)]

use nucleus::memory::FixedAllocator;
use nucleus::kernel::capabilities::CapabilitySet;

#[test]
fn test_fixed_allocator() {
    let mut alloc: FixedAllocator<u32, 4> = FixedAllocator::new();
    
    assert_eq!(alloc.remaining(), 4);
    assert!(alloc.allocate(1).is_ok());
    assert_eq!(alloc.remaining(), 3);
}

#[test]
fn test_capabilities() {
    let caps = CapabilitySet::new();
    
    assert!(caps.can_load_muscle());
    assert!(caps.can_emit_update());
}
