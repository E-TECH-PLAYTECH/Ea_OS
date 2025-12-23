//! Error types for the Eä muscle ecosystem

use core::fmt;
use thiserror::Error;

/// Main error type for muscle operations
#[derive(Error, Debug, Clone)]
pub enum MuscleError {
    /// Cryptographic operation failed
    #[error("cryptographic operation failed: {0}")]
    Crypto(String),

    /// Muscle blob is invalid or tampered with
    #[error("invalid or tampered muscle blob")]
    InvalidBlob,

    /// Resource limits exceeded
    #[error("resource limits exceeded")]
    ResourceExhausted,

    /// Isolation failure - sandbox violation
    #[error("isolation failure")]
    IsolationFailure,

    /// Random number generation failed
    #[error("random number generation failed")]
    RngFailure,

    /// Muscle is malformed
    #[error("malformed muscle organelle")]
    MalformedOrganelle,

    /// Missing entry point
    #[error("missing entry point")]
    MissingEntryPoint,

    /// WebAssembly trap occurred
    #[error("wasm trap: {0}")]
    Trap(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(String),

    /// Custom error
    #[error("muscle error: {0}")]
    Custom(String),
}

impl From<core::str::Utf8Error> for MuscleError {
    fn from(err: core::str::Utf8Error) -> Self {
        MuscleError::Custom(format!("UTF-8 error: {}", err))
    }
}

impl From<alloc::string::FromUtf8Error> for MuscleError {
    fn from(err: alloc::string::FromUtf8Error) -> Self {
        MuscleError::Custom(format!("UTF-8 conversion error: {}", err))
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for MuscleError {
    fn from(err: std::io::Error) -> Self {
        MuscleError::Io(err.to_string())
    }
}

#[cfg(feature = "std")]
impl From<wasmtime::Error> for MuscleError {
    fn from(err: wasmtime::Error) -> Self {
        MuscleError::Trap(err.to_string())
    }
}

#[cfg(feature = "std")]
impl From<wasmtime::Trap> for MuscleError {
    fn from(trap: wasmtime::Trap) -> Self {
        MuscleError::Trap(trap.to_string())
    }
}

/// Result type alias for muscle operations
pub type MuscleResult<T> = core::result::Result<T, MuscleError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let error = MuscleError::InvalidBlob;
        assert_eq!(format!("{}", error), "invalid or tampered muscle blob");

        let error = MuscleError::ResourceExhausted;
        assert_eq!(format!("{}", error), "resource limits exceeded");

        let error = MuscleError::Custom("test error".to_string());
        assert_eq!(format!("{}", error), "muscle error: test error");
    }

    #[test]
    fn test_error_conversions() {
        let utf8_err = core::str::from_utf8(&[0xFF]).unwrap_err();
        let muscle_err: MuscleError = utf8_err.into();
        assert!(matches!(muscle_err, MuscleError::Custom(_)));
    }
}
