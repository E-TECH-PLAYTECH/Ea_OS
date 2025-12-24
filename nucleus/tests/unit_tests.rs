#![cfg(test)]

// Use re-exported types from kernel module
use nucleus::kernel::CapabilitySet;
use nucleus::memory::FixedAllocator;

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

#[test]
fn test_syscalls() {
    use nucleus::kernel::MuscleNucleus;
    use nucleus::syscalls::{Syscall, SyscallArgs, SyscallHandler};

    let mut nucleus = MuscleNucleus::new();
    let args = SyscallArgs {
        arg0: 10,
        arg1: 0,
        arg2: 0,
    };

    // Test MuscAlloc
    let res = nucleus.handle_syscall(Syscall::MuscAlloc, args);
    assert!(res.is_ok());
}
