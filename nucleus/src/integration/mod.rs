mod lattice;
mod attestation;
mod symbiote;

pub use lattice::LatticeStream;
pub use ea_ledger::MuscleUpdate as LatticeUpdate; // Alias for compatibility
pub use attestation::HardwareAttestation;
pub use symbiote::{SymbioteInterface, SealedBlob, Heartbeat};
