use crate::{MAX_MUSCLES, NucleusError, Result};

/// Fixed priorities matching Eä design
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Min = 0,
    Low = 85,
    Normal = 170,
    High = 255,
}

impl Priority {
    pub const MAX: Self = Self::High;
}

/// Fixed-size scheduler with compile-time analysis
#[derive(Debug)]
pub struct Scheduler {
    schedule: [Option<usize>; 256], // Muscle slots by priority
    current_slot: u8,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            schedule: [None; 256],
            current_slot: 0,
        }
    }
    
    /// Schedule a muscle at given priority
    pub fn schedule(&mut self, muscle_slot: usize, priority: Priority) -> Result<()> {
        if muscle_slot >= MAX_MUSCLES {
            return Err(NucleusError::CapacityExceeded);
        }
        
        let priority_val = priority as u8;
        self.schedule[priority_val as usize] = Some(muscle_slot);
        Ok(())
    }
    
    /// Execute next scheduled muscle
    pub fn execute_next(&mut self) {
        // Round-robin within priority levels
        for priority in (0..=255).rev() {
            if let Some(slot) = self.schedule[priority as usize] {
                // In production, this would context switch to muscle
                self.execute_muscle(slot);
                break;
            }
        }
        
        self.current_slot = self.current_slot.wrapping_add(1);
    }
    
    /// Execute a specific muscle
    fn execute_muscle(&self, slot: usize) {
        // Muscle execution would happen here
        // For now, just increment execution counter
        unsafe {
            static mut EXEC_COUNTS: [u64; MAX_MUSCLES] = [0; MAX_MUSCLES];
            if slot < MAX_MUSCLES {
                EXEC_COUNTS[slot] += 1;
            }
        }
    }
}
