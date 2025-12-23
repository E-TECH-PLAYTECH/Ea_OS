#![no_std]

extern crate alloc;

pub mod crypto;
pub mod errors;
pub mod muscle_loader;
pub mod uart;

// Panic handler for cdylib target (no_std)
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}
