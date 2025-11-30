use crate::integration::SymbioteInterface;

pub struct TimerRule;

impl TimerRule {
    pub const fn new() -> Self {
        Self
    }
    
    pub fn execute(&self, symbiote: &SymbioteInterface, counter: u64) -> Option<SealedBlob> {
        let heartbeat = Heartbeat {
            muscle_id: 0xFFFF_FFFF_FFFF_FFFF, // Symbiote ID
            version: symbiote.version(),
            counter,
        };
        
        symbiote.seal_heartbeat(heartbeat)
    }
}

pub struct Heartbeat {
    pub muscle_id: u64,
    pub version: u32,
    pub counter: u64,
}
