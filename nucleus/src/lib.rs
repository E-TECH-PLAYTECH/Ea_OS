#![no_std]
#![feature(const_mut_refs)]
#![feature(const_fn_trait_bound)]

//! Muscle Nucleus - The first true biological kernel
//! 
//! 8 KiB of pure life with fixed-size, capability-based security
//! and compile-time verified rules.

pub mod kernel;
pub mod rules;
pub mod memory;
pub mod integration;

pub use kernel::MuscleNucleus;
pub use rules::{RuleEngine, RuleId};
pub use memory::FixedAllocator;
pub use integration::{LatticeStream, HardwareAttestation, SymbioteInterface};

/// Core error types for the nucleus
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NucleusError {
    CapacityExceeded,
    InvalidCapability,
    RuleViolation,
    VerificationFailed,
    MemoryFault,
}

/// Result type for nucleus operations
pub type Result<T> = core::result::Result<T, NucleusError>;

/// Fixed-size constants matching Eä architecture
pub const KERNEL_SIZE: usize = 8192; // 8KiB total kernel
pub const MAX_MUSCLES: usize = 16;
pub const MAX_UPDATES: usize = 16;
pub const SCHEDULE_SLOTS: usize = 256;
pub const SYMBIOTE_ID: u64 = 0xFFFF_FFFF_FFFF_FFFF; // Highest priority
