mod attestation;
mod lattice;
mod symbiote;

pub use attestation::HardwareAttestation;
pub use ea_ledger::MuscleUpdate as LatticeUpdate; // Alias for compatibility
pub use lattice::LatticeStream;
pub use symbiote::{Heartbeat, SealedBlob, SymbioteInterface};
