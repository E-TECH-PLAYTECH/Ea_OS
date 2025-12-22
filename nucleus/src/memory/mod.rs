mod fixed_alloc;

pub use fixed_alloc::FixedAllocator;

pub mod page_alloc {
    use core::alloc::Layout;
    use core::cell::UnsafeCell;

    #[derive(Debug)]
    pub struct PageAllocator {
        start: usize,
        end: usize,
        current: UnsafeCell<usize>,
    }

    impl PageAllocator {
        pub const fn new(start: usize, end: usize) -> Self {
            Self {
                start,
                end,
                current: UnsafeCell::new(start),
            }
        }
        
        pub unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let current = self.current.get();
            let aligned = (*current + layout.align() - 1) & !(layout.align() - 1);
            let new_current = aligned + layout.size();
            
            if new_current > self.end {
                core::ptr::null_mut()
            } else {
                *current = new_current;
                aligned as *mut u8
            }
        }
        
        pub unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
            // Bump allocator doesn't free
        }

        pub fn current_top(&self) -> usize {
            unsafe { *self.current.get() }
        }
    }
}
pub mod manager {
    use alloc::collections::BTreeMap;
    use crate::kernel::Capability;
    use crate::NucleusError;
    use super::page_alloc::PageAllocator;
    use core::alloc::Layout;

    // 1MB Heap for Muscles
    const HEAP_START: usize = 0x4000_0000;
    const HEAP_SIZE: usize = 1024 * 1024; 

    #[derive(Debug)]
    pub struct MemoryManager {
        allocator: PageAllocator,
        muscle_pages: BTreeMap<u64, (usize, usize)>, // muscle_id -> (start_addr, page_count)
        capability_regions: BTreeMap<[u8; 32], (usize, usize)>, // cap_key -> (start, len)
    }

    impl MemoryManager {
        pub fn new() -> Self {
            Self {
                allocator: PageAllocator::new(HEAP_START, HEAP_START + HEAP_SIZE),
                muscle_pages: BTreeMap::new(),
                capability_regions: BTreeMap::new(),
            }
        }
        
        pub fn map_muscle(&mut self, muscle_id: u64, pages: usize) -> Result<usize, NucleusError> {
            let size = pages * 4096;
            let layout = Layout::from_size_align(size, 4096).map_err(|_| NucleusError::MemoryFault)?;
            
            let ptr = unsafe { self.allocator.alloc(layout) };
            if ptr.is_null() {
                return Err(NucleusError::CapacityExceeded);
            }

            let addr = ptr as usize;
            self.muscle_pages.insert(muscle_id, (addr, pages));
            Ok(addr)
        }
        
        pub fn create_shared_region(&mut self, cap: Capability, size: usize) -> Result<usize, NucleusError> {
            let layout = Layout::from_size_align(size, 4096).map_err(|_| NucleusError::MemoryFault)?;
            let ptr = unsafe { self.allocator.alloc(layout) };
            
            if ptr.is_null() {
                return Err(NucleusError::CapacityExceeded);
            }

            let addr = ptr as usize;
            self.capability_regions.insert(cap.key, (addr, size));
            Ok(addr)
        }

        pub fn get_muscle_region(&self, muscle_id: u64) -> Option<(usize, usize)> {
            self.muscle_pages.get(&muscle_id).copied()
        }
    }
}
