// referee/src/errors.rs
// Eä Error Types v5.0 — Comprehensive error handling

#[derive(Debug, Clone, Copy)]
pub enum BootError {
    MasterKeyInvalid,
    MemoryAllocationFailed,
    MuscleLoadFailed,
    UartInitializationFailed,
    SystemTableInvalid,
}

#[derive(Debug, Clone, Copy)]
pub enum MuscleError {
    DecryptionFailed,
    IntegrityCheckFailed,
    InvalidFormat,
    ArchitectureMismatch,
    ExecutionFailed,
}

impl From<MuscleError> for BootError {
    fn from(error: MuscleError) -> Self {
        match error {
            MuscleError::DecryptionFailed => BootError::MuscleLoadFailed,
            MuscleError::IntegrityCheckFailed => BootError::MuscleLoadFailed,
            MuscleError::InvalidFormat => BootError::MuscleLoadFailed,
            MuscleError::ArchitectureMismatch => BootError::MuscleLoadFailed,
            MuscleError::ExecutionFailed => BootError::MuscleLoadFailed,
        }
    }
}

impl core::fmt::Display for BootError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BootError::MasterKeyInvalid => write!(f, "Master key invalid or corrupted"),
            BootError::MemoryAllocationFailed => write!(f, "Memory allocation failed"),
            BootError::MuscleLoadFailed => write!(f, "Muscle loading failed"),
            BootError::UartInitializationFailed => write!(f, "UART initialization failed"),
            BootError::SystemTableInvalid => write!(f, "UEFI system table invalid"),
        }
    }
}

impl core::fmt::Display for MuscleError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MuscleError::DecryptionFailed => write!(f, "Muscle decryption failed"),
            MuscleError::IntegrityCheckFailed => write!(f, "Muscle integrity check failed"),
            MuscleError::InvalidFormat => write!(f, "Invalid muscle format"),
            MuscleError::ArchitectureMismatch => write!(f, "Architecture mismatch"),
            MuscleError::ExecutionFailed => write!(f, "Muscle execution failed"),
        }
    }
}
