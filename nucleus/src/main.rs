#![no_std]
#![no_main]

extern crate alloc;

use core::panic::PanicInfo;
use nucleus::kernel::MuscleNucleus;
use linked_list_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Initialize heap
    unsafe {
        ALLOCATOR
            .lock()
            .init(0x4000_0000 as *mut u8, 1024 * 1024); // 1MB Heap
    }

    // Initialize the biological kernel
    let mut nucleus = MuscleNucleus::new();
    
    // Execute boot rule - this never returns
    nucleus.execute_boot_rule();
    
    loop {}
}
