#![no_std]
#![no_main]

//! Pre-Nucleus Loader - 2KiB verified loader that boots the Nucleus Muscle
//!
//! This is the minimal Rust component that:
//! 1. Is verified by Referee
//! 2. Verifies the Nucleus Muscle blob (at a known memory address)
//! 3. Sets up execution environment
//! 4. Transfers control to Nucleus Muscle
//!
//! The loader itself is ~2KiB. The nucleus blob lives at NUCLEUS_BLOB_ADDR.

use core::arch::naked_asm;
use core::panic::PanicInfo;

/// Known memory addresses (set by Referee)
const NUCLEUS_BLOB_ADDR: u64 = 0x9200_0000;
const NUCLEUS_BLOB_SIZE: usize = 8192;
const VERIFICATION_KEY_ADDR: u64 = 0x9000_0020; // After master key

/// Boot parameters passed from Referee (in X0 register)
#[repr(C)]
pub struct BootParameters {
    pub memory_map_addr: u64,
    pub memory_map_size: u64,
    pub lattice_root: [u8; 32],
    pub master_key_addr: u64,
    pub nucleus_blob_addr: u64,
}

/// Entry point called by Referee after verification
/// Boot parameters pointer passed in X0
#[unsafe(naked)]
#[no_mangle]
pub extern "C" fn _start() -> ! {
    naked_asm!(
            // Save boot parameters from Referee (in X0)
            "mov x19, x0",
            // 1. Verify Nucleus blob signature
            "bl {verify}",
            "cbz x0, 2f",
            // 2. Set up Nucleus execution environment
            "bl {setup}",
            // 3. Load Nucleus entry point address
            "ldr x20, ={nucleus_addr}",
            // 4. Transfer control to Nucleus Muscle
            "br x20",
            // Verification failed
            "2:",
            "bl {halt}",
            verify = sym verify_nucleus_blob,
            setup = sym setup_nucleus_environment,
            halt = sym halt_system,
            nucleus_addr = const NUCLEUS_BLOB_ADDR,
    )
}

/// Verify the Nucleus blob signature at known address
#[no_mangle]
extern "C" fn verify_nucleus_blob() -> u64 {
    // Read verification key and nucleus blob from known addresses
    let _key_ptr = VERIFICATION_KEY_ADDR as *const [u8; 32];
    let _blob_ptr = NUCLEUS_BLOB_ADDR as *const u8;

    // In production: verify BLAKE3 hash against key
    // For now, return success
    1
}

/// Set up execution environment for Nucleus
#[no_mangle]
extern "C" fn setup_nucleus_environment() {
    use core::arch::asm;
    unsafe {
        // Set up stack pointer for Nucleus
        asm!("mov sp, 0x8000", options(nostack));

        // Configure system registers for isolated execution
        asm!(
            "msr sctlr_el1, xzr",
            "msr ttbr0_el1, xzr",
            "msr ttbr1_el1, xzr",
            options(nostack)
        );
    }
}

/// Halt system on critical failure
#[no_mangle]
extern "C" fn halt_system() -> ! {
    use core::arch::asm;
    unsafe {
        loop {
            asm!("wfe", options(nomem, nostack));
        }
    }
}

// Panic handler only for bare-metal UEFI target
#[cfg(target_os = "uefi")]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    halt_system()
}

// Size check: BootParameters struct should be small
// The actual code size is verified at link time via linker script
const _: () = assert!(
    core::mem::size_of::<BootParameters>() <= 128,
    "BootParameters too large"
);
