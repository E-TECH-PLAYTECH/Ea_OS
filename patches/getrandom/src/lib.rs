#![no_std]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;
use core::num::NonZeroU32;

/// Error type compatible with rand_core 0.6 expectations.
/// Wraps an error code for API compatibility.
#[derive(Debug, Clone, Copy)]
pub struct Error(NonZeroU32);

impl Error {
    /// Internal error code used by rand_core.
    pub const INTERNAL_START: u32 = 1 << 31;
    /// Custom error code marker.
    pub const CUSTOM_START: u32 = 1 << 31;
    /// Unknown error code fallback.
    const UNKNOWN: NonZeroU32 = unsafe { NonZeroU32::new_unchecked(Self::INTERNAL_START) };

    /// Create a new error from a code.
    pub fn new_custom(code: u32) -> Self {
        Self(NonZeroU32::new(code).unwrap_or(Self::UNKNOWN))
    }

    /// Get the raw error code (for rand_core compatibility).
    pub fn code(&self) -> NonZeroU32 {
        self.0
    }

    /// Get the raw OS error if applicable.
    pub fn raw_os_error(&self) -> Option<i32> {
        None
    }
}

impl From<NonZeroU32> for Error {
    fn from(code: NonZeroU32) -> Self {
        Self(code)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "getrandom error code: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// When std is enabled, use real random bytes from the OS.
/// In no_std mode, fills with zeros (deterministic).
#[cfg(feature = "std")]
pub fn getrandom(dest: &mut [u8]) -> Result<(), Error> {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(dest))
        .map_err(|_| Error::new_custom(1))
}

#[cfg(not(feature = "std"))]
pub fn getrandom(dest: &mut [u8]) -> Result<(), Error> {
    for byte in dest.iter_mut() {
        *byte = 0;
    }
    Ok(())
}

/// Placeholder macro with the same signature as the upstream crate so existing
/// uses of `register_custom_getrandom!` still compile.
#[macro_export]
macro_rules! register_custom_getrandom {
    ($func:path) => {
        const _: () = {
            let _ = $func;
        };
    };
}
