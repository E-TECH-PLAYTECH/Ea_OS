#![no_std]

#[cfg(feature = "std")]
extern crate std;

use core::fmt;

/// Basic error placeholder so downstream crates can handle errors.
#[derive(Debug, Clone, Copy)]
pub struct Error;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "getrandom stub error")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// Fills the buffer with deterministic bytes (zeros). This ensures the symbol is
/// available on no-std targets without relying on platform APIs.
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
