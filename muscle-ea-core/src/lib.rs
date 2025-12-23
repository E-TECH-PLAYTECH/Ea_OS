#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![deny(missing_docs, clippy::all, clippy::pedantic)]
#![doc = r#"
Core biological substrate for Eä Muscle ecosystem.

Provides shared types, traits, and cryptographic primitives that enable
the biological computing paradigm where every program is a living cell.
"#]

extern crate alloc;

pub mod biology;
pub mod crypto;
pub mod error;
pub mod runtime;

// Re-export main types for easy access
pub use biology::{MuscleSalt, SealedBlob, SuccessorKey};
pub use error::MuscleError;
pub use runtime::{Muscle, MuscleContext, MuscleOutput, MuscleSuccessor};

/// Core biological constants for the Eä ecosystem
pub mod constants {
    /// Maximum size of a muscle blob in bytes
    pub const MAX_MUSCLE_SIZE: usize = 8192; // 8KiB

    /// Maximum number of successor muscles
    pub const MAX_SUCCESSORS: usize = 16;

    /// Standard salt size for muscle derivation
    pub const SALT_SIZE: usize = 16;

    /// Key size for all cryptographic operations
    pub const KEY_SIZE: usize = 32;
}

/// Prelude for easy importing of core functionality
pub mod prelude {
    pub use super::constants::*;
    pub use super::{
        Muscle, MuscleContext, MuscleError, MuscleOutput, MuscleSalt, MuscleSuccessor, SealedBlob,
        SuccessorKey,
    };
    pub use rand_core::{CryptoRng, RngCore};
    pub use zeroize::Zeroizing;
}
