// referee/src/main.rs
// Eä Referee v5.0 — Secure UEFI Bootloader with v5.0 Crypto Integration
#![no_std]
#![no_main]
#![feature(abi_efiapi)]

use uefi::prelude::*;
use uefi::table::boot::BootServices;

mod crypto;
mod muscle_loader;
mod uart;

use crate::muscle_loader::{load_muscle, LoadedMuscle};
use crate::uart::Uart;

const N_MUSCLES: usize = 50;
const MUSCLE_BUNDLE_BASE: u64 = 0x9100_0000;
const MUSCLE_SIZE: usize = 8192;

/// Global system state
struct RefereeState {
    muscles: [Option<LoadedMuscle>; N_MUSCLES],
    loaded_count: usize,
}

impl RefereeState {
    const fn new() -> Self {
        Self {
            muscles: [None; N_MUSCLES],
            loaded_count: 0,
        }
    }
}

static mut STATE: RefereeState = RefereeState::new();

#[entry]
fn efi_main(_image: Handle, system_table: SystemTable<Boot>) -> Status {
    // Initialize UEFI services
    uefi_services::init(&system_table).unwrap_success();

    let boot_services = system_table.boot_services();
    let mut uart = Uart::new();

    // Initialize UART for logging
    if let Err(e) = uart.init() {
        log(&uart, "ERROR", &format!("UART init failed: {}", e));
        return Status::LOAD_ERROR;
    }

    log(&uart, "INFO", "Eä Referee v5.0 awakening...");

    // Load master chaos key
    let master_key = match load_master_key(boot_services) {
        Ok(key) => {
            log(&uart, "INFO", "Chaos master key acquired");
            key
        }
        Err(e) => {
            log(&uart, "FATAL", &format!("Master key load failed: {}", e));
            return Status::LOAD_ERROR;
        }
    };

    // Load and validate all muscles
    if let Err(e) = load_all_muscles(boot_services, &master_key, &mut uart) {
        log(&uart, "FATAL", &format!("Muscle loading failed: {}", e));
        return Status::LOAD_ERROR;
    }

    log(
        &uart,
        "INFO",
        &format!("{} muscles alive — Eä breathes", unsafe {
            STATE.loaded_count
        }),
    );

    // Transfer control to scheduler
    run_scheduler(&mut uart)
}

/// Load master key from fixed memory location
fn load_master_key(_boot_services: &BootServices) -> Result<[u8; 32], &'static str> {
    let key_ptr = 0x9000_0000 as *const u8;

    // Verify key header
    let header = unsafe { core::slice::from_raw_parts(key_ptr, 8) };
    if header != b"EaKEYv5" {
        return Err("invalid key header");
    }

    // Extract key
    let mut key = [0u8; 32];
    unsafe {
        core::ptr::copy_nonoverlapping(key_ptr.add(8), key.as_mut_ptr(), 32);
    }

    Ok(key)
}

/// Load all muscles from bundle
fn load_all_muscles(
    boot_services: &BootServices,
    master_key: &[u8; 32],
    uart: &mut Uart,
) -> Result<(), &'static str> {
    for i in 0..N_MUSCLES {
        let muscle_addr = MUSCLE_BUNDLE_BASE + (i * MUSCLE_SIZE) as u64;

        // Read muscle blob from memory
        let blob_data =
            unsafe { core::slice::from_raw_parts(muscle_addr as *const u8, MUSCLE_SIZE) };

        // Skip empty slots
        if blob_data.iter().all(|&b| b == 0) {
            continue;
        }

        // Load and validate muscle
        match load_muscle(boot_services, master_key, blob_data, i) {
            Ok(loaded_muscle) => {
                unsafe {
                    STATE.muscles[i] = Some(loaded_muscle);
                    STATE.loaded_count += 1;
                }
                log(
                    uart,
                    "INFO",
                    &format!("Muscle '{}' loaded successfully", loaded_muscle.name),
                );
            }
            Err(e) => {
                log(
                    uart,
                    "WARN",
                    &format!("Muscle {} failed to load: {:?}", i, e),
                );
                // Continue with other muscles (graceful degradation)
            }
        }
    }

    if unsafe { STATE.loaded_count } == 0 {
        return Err("no muscles loaded successfully");
    }

    Ok(())
}

/// Simple round-robin scheduler
fn run_scheduler(uart: &mut Uart) -> ! {
    log(uart, "INFO", "Starting muscle scheduler...");

    let mut current_muscle = 0;
    let mut execution_count = 0;

    loop {
        // Find next available muscle
        let muscle_idx = current_muscle % N_MUSCLES;

        if let Some(muscle) = unsafe { &STATE.muscles[muscle_idx] } {
            execution_count += 1;

            // Log every 1000 executions
            if execution_count % 1000 == 0 {
                log(uart, "DEBUG", &format!("Executions: {}", execution_count));
            }

            // Execute muscle
            unsafe {
                execute_muscle(muscle.entry_point);
            }
        }

        current_muscle += 1;

        // Small delay to prevent busyloop
        unsafe {
            let bs = uefi::table::SystemTable::<uefi::table::Boot>::current().boot_services();
            bs.stall(1000);
        }
    }
}

/// Execute muscle at given entry point
unsafe fn execute_muscle(entry_point: u64) {
    // For AArch64
    #[cfg(target_arch = "aarch64")]
    core::arch::asm!(
        "blr {}",
        in(reg) entry_point,
        options(noreturn)
    );

    // For x86_64
    #[cfg(target_arch = "x86_64")]
    core::arch::asm!(
        "call {}",
        in(reg) entry_point,
        options(noreturn)
    );
}

/// Log message via UART
fn log(uart: &mut Uart, level: &str, message: &str) {
    let _ = uart.write_str(&format!("[{}] {}\n", level, message));
}
