use crate::NucleusError;

/// Compile-time capability system
#[derive(Debug, Clone, Copy)]
pub struct CapabilitySet {
    load_muscle: bool,
    schedule: u8,    // Bitmap of allowed priorities
    emit_update: usize, // Max updates allowed
}

impl CapabilitySet {
    pub const fn new() -> Self {
        Self {
            load_muscle: true,
            schedule: 0b1111_1111, // Allow all priorities
            emit_update: 16,       // Max 16 updates
        }
    }
    
    pub const fn can_load_muscle(&self) -> bool {
        self.load_muscle
    }
    
    pub const fn can_schedule(&self, priority: u8) -> bool {
        (self.schedule & (1 << (priority >> 5))) != 0
    }
    
    pub const fn can_emit_update(&self) -> bool {
        self.emit_update > 0
    }
    
    pub fn use_emit_capability(&mut self) -> Result<(), NucleusError> {
        if self.emit_update == 0 {
            Err(NucleusError::InvalidCapability)
        } else {
            self.emit_update -= 1;
            Ok(())
        }
    }
}
