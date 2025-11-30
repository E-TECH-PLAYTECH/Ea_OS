use crate::NucleusError;

/// Fixed-size allocator for no-std environments
pub struct FixedAllocator<T, const N: usize> {
    buffer: [Option<T>; N],
    count: usize,
}

impl<T, const N: usize> FixedAllocator<T, N> {
    pub const fn new() -> Self {
        Self {
            buffer: [None; N],
            count: 0,
        }
    }
    
    pub fn allocate(&mut self, item: T) -> Result<(), ()> {
        if self.count >= N {
            return Err(());
        }
        
        for slot in &mut self.buffer {
            if slot.is_none() {
                *slot = Some(item);
                self.count += 1;
                return Ok(());
            }
        }
        
        Err(())
    }
    
    pub fn deallocate(&mut self, index: usize) -> Option<T> {
        if index < N {
            if let Some(item) = self.buffer[index].take() {
                self.count -= 1;
                return Some(item);
            }
        }
        None
    }
    
    pub const fn remaining(&self) -> usize {
        N - self.count
    }
    
    pub const fn is_full(&self) -> bool {
        self.count >= N
    }
}
