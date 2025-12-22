//! Muscle Nucleus - The first true biological kernel
//! 
//! 8 KiB of pure life with fixed-size, capability-based security
//! and compile-time verified rules.

#![no_std]
#![feature(const_mut_refs)]
#![feature(const_fn_trait_bound)]

extern crate alloc;

pub mod kernel;
pub mod rules;
pub mod memory;
pub mod integration;

pub mod syscalls {
    use crate::NucleusError;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u64)]
    pub enum Syscall {
        // Memory (0x100 range)
        MuscAlloc = 0x100,
        MuscFree = 0x101,
        MuscMap = 0x102,

        // Lattice (0x200 range)
        LatticeRead = 0x200,
        LatticeWrite = 0x201,
        LatticeVerify = 0x202,

        // Capability (0x300 range)
        CapDerive = 0x300,
        CapDelegate = 0x301,
        CapRevoke = 0x302,

        // IPC (0x400 range)
        ChannelCreate = 0x400,
        ChannelSend = 0x401,
        ChannelRecv = 0x402,
    }

    impl Syscall {
        pub fn from_u64(val: u64) -> Option<Self> {
            match val {
                0x100 => Some(Syscall::MuscAlloc),
                0x101 => Some(Syscall::MuscFree),
                0x102 => Some(Syscall::MuscMap),
                0x200 => Some(Syscall::LatticeRead),
                0x201 => Some(Syscall::LatticeWrite),
                0x202 => Some(Syscall::LatticeVerify),
                0x300 => Some(Syscall::CapDerive),
                0x301 => Some(Syscall::CapDelegate),
                0x302 => Some(Syscall::CapRevoke),
                0x400 => Some(Syscall::ChannelCreate),
                0x401 => Some(Syscall::ChannelSend),
                0x402 => Some(Syscall::ChannelRecv),
                _ => None,
            }
        }
    }

    #[repr(C)]
    pub struct SyscallArgs {
        pub arg0: usize,
        pub arg1: usize,
        pub arg2: usize,
    }

    pub type SyscallResult = Result<usize, NucleusError>;

    pub trait SyscallHandler {
        fn handle_syscall(&mut self, syscall: Syscall, args: SyscallArgs) -> SyscallResult;
    }
}

pub mod capability {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Capability {
        pub key: [u8; 32],
        pub rights: Rights,
        pub object_type: ObjectType,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub struct Rights(pub u8);

    impl Rights {
        pub const READ: Self = Self(0b0001);
        pub const WRITE: Self = Self(0b0010);
        pub const EXECUTE: Self = Self(0b0100);
        pub const DELEGATE: Self = Self(0b1000);

        pub fn contains(&self, other: Self) -> bool {
            (self.0 & other.0) == other.0
        }
        
        pub fn bits(&self) -> u8 {
            self.0
        }
    }

    impl core::ops::BitOr for Rights {
        type Output = Self;
        fn bitor(self, rhs: Self) -> Self {
            Self(self.0 | rhs.0)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    pub enum ObjectType {
        MemoryRegion,
        Channel,
        File,
        LatticeObject,
    }
}

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
