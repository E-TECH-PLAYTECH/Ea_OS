#![cfg_attr(all(not(test), not(feature = "std")), no_std)]

extern crate alloc;

pub mod crypto;
pub mod errors;
pub mod muscle_loader;
pub mod uart;

// Panic handler for no_std builds only (UEFI target, not test, not std feature)
// Only enabled when building as the final target, not as a dependency
#[cfg(all(
    not(test),
    not(feature = "std"),
    target_os = "uefi"
))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
