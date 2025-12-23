use crate::integration::SymbioteInterface;
use crate::integration::{Heartbeat, SealedBlob};

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
